/*!
 * Compact copy-by-value betting & incremental FNV-1a infoset hash state (`TrainState`)
 * and zero-modulo-bias 9-card bitmask dealer for parallel Hold'em MCCFR training.
 */

use rand::RngExt;
use rand::rngs::SmallRng;
use std::cell::Cell;
use std::fmt::Write as FmtWrite;

use crate::cfr::abstraction::{
    FNV_OFFSET_BASIS, action_to_char, append_texture_to_key, compact_board_texture_char,
    fnv1a_combine, mix_texture_hash, postflop_equity_bucket, postflop_equity_bucket_with_score,
    preflop_bucket,
};
use crate::cfr::flat_table::PF_SEQ_CACHE_STRIDE;
use crate::game::config::{
    ALL_IN, BIG_BLIND, CALL_CHECK, FOLD, HOLDEM_NUM_ACTIONS, MAX_RAISES_PER_ROUND,
    MAX_ROUND_ACTIONS, PREFLOP_OPEN_MIN_TOTAL, RAISE_HALF_POT, RAISE_HALF_POT_FLOOR_DRY,
    RAISE_HALF_POT_FLOOR_RIVER, RAISE_HALF_POT_FLOOR_TURN, RAISE_HALF_POT_FLOOR_WET, RAISE_MIN,
    RAISE_MIN_AMT, RAISE_THIRD_POT, RAISE_THIRD_POT_FLOOR, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER,
    ROUND_TURN, SMALL_BLIND,
};
use crate::game::holdem::{ALL_52_CARDS, Card, evaluate_7cards};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StackTag {
    Deep,
    S15,
    S30,
}

impl StackTag {
    #[inline(always)]
    pub(crate) fn from_stack_chips(stack_size: i32) -> Self {
        let bb = stack_size / BIG_BLIND.max(1);
        if bb > 0 && bb <= 20 {
            Self::S15
        } else if bb > 0 && bb <= 45 {
            Self::S30
        } else {
            Self::Deep
        }
    }

    #[inline(always)]
    pub(crate) fn mix_hash(self, hash: u64) -> u64 {
        match self {
            Self::Deep => hash,
            Self::S15 => fnv1a_combine(
                fnv1a_combine(fnv1a_combine(fnv1a_combine(hash, b'S'), b'1'), b'5'),
                b'|',
            ),
            Self::S30 => fnv1a_combine(
                fnv1a_combine(fnv1a_combine(fnv1a_combine(hash, b'S'), b'3'), b'0'),
                b'|',
            ),
        }
    }

    #[inline(always)]
    pub(crate) fn prefix_str(self) -> &'static str {
        match self {
            Self::Deep => "",
            Self::S15 => "S15|",
            Self::S30 => "S30|",
        }
    }

    #[inline(always)]
    pub(crate) fn cache_bank(self) -> usize {
        match self {
            Self::Deep => 0,
            Self::S15 => 1,
            Self::S30 => 2,
        }
    }
}

/** Uniform zero-modulo-bias 9-card draw using a 64-bit occupancy mask and 6-bit word slicing. */
#[inline(always)]
pub(crate) fn deal_9_cards(rng: &mut SmallRng) -> ([[Card; 2]; 2], [Card; 5]) {
    let mut used = 0u64;
    let mut idx = [0usize; 9];
    let mut count = 0;
    while count < 9 {
        let mut w: u64 = rng.random();
        for _ in 0..10 {
            let c = (w & 63) as usize;
            w >>= 6;
            if c < 52 {
                let bit = 1u64 << c;
                if (used & bit) == 0 {
                    used |= bit;
                    idx[count] = c;
                    count += 1;
                    if count == 9 {
                        break;
                    }
                }
            }
        }
    }
    (
        [
            [ALL_52_CARDS[idx[0]], ALL_52_CARDS[idx[1]]],
            [ALL_52_CARDS[idx[2]], ALL_52_CARDS[idx[3]]],
        ],
        [
            ALL_52_CARDS[idx[4]],
            ALL_52_CARDS[idx[5]],
            ALL_52_CARDS[idx[6]],
            ALL_52_CARDS[idx[7]],
            ALL_52_CARDS[idx[8]],
        ],
    )
}

pub(crate) struct EpisodeCtx {
    pub(crate) hole: [[Card; 2]; 2],
    pub(crate) runout: [Card; 5],
    pub(crate) pf_bucket: [u8; 2],
    pub(crate) pf_name: [&'static str; 2],
    postflop: [[Cell<u8>; 2]; 3],
    river_scores: [Cell<u64>; 2],
    flop_tex: Cell<char>,
    sd_outcome: Cell<i8>,
    pub(crate) stack_size: i32,
    pub(crate) stack_floor_scaled: i64,
    pub(crate) stack_tag: StackTag,
    pub(crate) is_wet_flop: bool,
}

impl EpisodeCtx {
    #[inline(always)]
    pub(crate) fn new(hole: [[Card; 2]; 2], runout: [Card; 5], stack_size: i32) -> Self {
        let eff_stack = stack_size.max(BIG_BLIND * 2);
        let stack_floor = (eff_stack as f64 * 0.06).max(90.0);
        let (pf0, n0) = preflop_bucket(hole[0][0], hole[0][1]);
        let (pf1, n1) = preflop_bucket(hole[1][0], hole[1][1]);
        let is_wet_flop = runout[0].suit == runout[1].suit
            || runout[0].suit == runout[2].suit
            || runout[1].suit == runout[2].suit;
        Self {
            hole,
            runout,
            pf_bucket: [pf0 as u8, pf1 as u8],
            pf_name: [n0, n1],
            postflop: [
                [Cell::new(u8::MAX), Cell::new(u8::MAX)],
                [Cell::new(u8::MAX), Cell::new(u8::MAX)],
                [Cell::new(u8::MAX), Cell::new(u8::MAX)],
            ],
            river_scores: [Cell::new(u64::MAX), Cell::new(u64::MAX)],
            flop_tex: Cell::new('\0'),
            sd_outcome: Cell::new(i8::MIN),
            stack_size: eff_stack,
            stack_floor_scaled: (stack_floor * 1_000_000.0) as i64,
            stack_tag: StackTag::from_stack_chips(eff_stack),
            is_wet_flop,
        }
    }

    #[inline(always)]
    fn get_river_score(&self, player: usize) -> u64 {
        let cached = self.river_scores[player].get();
        if cached != u64::MAX {
            return cached;
        }
        let s = evaluate_7cards(&self.hole[player], &self.runout);
        self.river_scores[player].set(s);
        s
    }

    #[inline(always)]
    pub(crate) fn get_bucket(&self, round: u8, player: usize) -> u8 {
        if round <= ROUND_PREFLOP {
            return self.pf_bucket[player];
        }
        let s_idx = (round - 2) as usize;
        let cached = self.postflop[s_idx][player].get();
        if cached != u8::MAX {
            return cached;
        }
        let b = if round == ROUND_RIVER {
            let r_score = self.get_river_score(player);
            postflop_equity_bucket_with_score(&self.hole[player], &self.runout, r_score) as u8
        } else {
            let board_len = (round as usize) + 1;
            postflop_equity_bucket(&self.hole[player], &self.runout[..board_len]) as u8
        };
        self.postflop[s_idx][player].set(b);
        b
    }

    #[inline(always)]
    pub(crate) fn get_flop_texture(&self) -> char {
        let c = self.flop_tex.get();
        if c != '\0' {
            return c;
        }
        let tc = compact_board_texture_char(&self.runout[..3]);
        self.flop_tex.set(tc);
        tc
    }

    #[inline(always)]
    pub(crate) fn showdown_outcome(&self) -> i8 {
        let cached = self.sd_outcome.get();
        if cached != i8::MIN {
            return cached;
        }
        let s0 = self.get_river_score(0);
        let s1 = self.get_river_score(1);
        let out = if s0 > s1 {
            1
        } else if s1 > s0 {
            -1
        } else {
            0
        };
        self.sd_outcome.set(out);
        out
    }
}

#[inline(always)]
fn pack_coarse_actions(acts: &[u8]) -> u32 {
    let mut packed = 0u32;
    for (i, &a) in acts.iter().enumerate() {
        let code: u32 = match a {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        packed |= code << (i * 2);
    }
    packed
}

#[inline]
fn push_unpacked_coarse(out: &mut String, packed: u32, len: u8) {
    for i in 0..(len as usize) {
        let c = match (packed >> (i * 2)) & 3 {
            0 => 'f',
            1 => 'c',
            _ => 'r',
        };
        out.push(c);
    }
}

/** Compact copy-by-value betting & incremental FNV-1a hash state for hot-loop traversal. */
#[derive(Clone, Copy)]
pub struct TrainState {
    prefix_hash: u64,
    pub contributions: [i32; 2],
    pf_packed: u32,
    f_packed: u32,
    pf_seq: u16,
    cur_acts: [u8; MAX_ROUND_ACTIONS],
    cur_len: u8,
    pf_len: u8,
    f_len: u8,
    pub round: u8,
    pub current_player: u8,
    pub raises_this_round: u8,
}

pub enum StepResult {
    Continue(TrainState),
    FoldWin { loser_contrib: i32, loser: usize },
    Showdown { contributions: [i32; 2] },
}

impl TrainState {
    #[inline(always)]
    pub fn new_hand(stack_size: i32) -> Self {
        let eff = stack_size.max(BIG_BLIND * 2);
        Self {
            prefix_hash: FNV_OFFSET_BASIS,
            contributions: [SMALL_BLIND.min(eff), BIG_BLIND.min(eff)],
            pf_packed: 0,
            f_packed: 0,
            pf_seq: 0,
            cur_acts: [0; MAX_ROUND_ACTIONS],
            cur_len: 0,
            pf_len: 0,
            f_len: 0,
            round: ROUND_PREFLOP,
            current_player: 0,
            raises_this_round: 0,
        }
    }

    #[inline(always)]
    pub fn legal_actions_buf(&self, stack_cap: i32, out: &mut [u8; HOLDEM_NUM_ACTIONS]) -> usize {
        let curr = self.current_player as usize;
        let opp = 1 - curr;
        let my_contrib = self.contributions[curr];
        let diff = self.contributions[opp] - my_contrib;
        let raise_capped = self.raises_this_round >= MAX_RAISES_PER_ROUND;
        let pot = (self.contributions[0] + self.contributions[1]).max(1);
        let rem_stack = (stack_cap - my_contrib).max(0);
        let call_need = diff.max(0);
        let rem_after_call = (rem_stack - call_need).max(0);
        let pot_after_call = pot + call_need;
        let allow_allin = if self.round == ROUND_PREFLOP {
            stack_cap <= BIG_BLIND * 25
                || self.raises_this_round >= 2
                || rem_after_call <= pot_after_call * 4
        } else {
            rem_after_call <= pot_after_call * 3
        };

        if my_contrib >= stack_cap || self.contributions[opp] >= stack_cap {
            if diff > 0 {
                out[0] = FOLD;
                out[1] = CALL_CHECK;
                2
            } else {
                out[0] = CALL_CHECK;
                1
            }
        } else if raise_capped {
            if diff > 0 {
                out[0] = FOLD;
                out[1] = CALL_CHECK;
                out[2] = ALL_IN;
                3
            } else {
                out[0] = CALL_CHECK;
                out[1] = ALL_IN;
                2
            }
        } else if diff > 0 {
            out[0] = FOLD;
            out[1] = CALL_CHECK;
            out[2] = RAISE_MIN;
            out[3] = RAISE_THIRD_POT;
            out[4] = RAISE_HALF_POT;
            if allow_allin {
                out[5] = ALL_IN;
                HOLDEM_NUM_ACTIONS
            } else {
                HOLDEM_NUM_ACTIONS - 1
            }
        } else {
            out[0] = CALL_CHECK;
            out[1] = RAISE_MIN;
            out[2] = RAISE_THIRD_POT;
            out[3] = RAISE_HALF_POT;
            if allow_allin {
                out[4] = ALL_IN;
                5
            } else {
                4
            }
        }
    }

    #[inline(always)]
    pub(crate) fn preflop_cache_idx(&self, bucket: u8, stack_tag: StackTag) -> Option<usize> {
        if self.round == ROUND_PREFLOP && self.pf_seq != u16::MAX {
            let bank = stack_tag.cache_bank() * 169 + usize::from(bucket);
            Some(bank * PF_SEQ_CACHE_STRIDE + usize::from(self.pf_seq))
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn infoset_hash(&self, bucket: u8, tex_char: Option<char>, stack_chips: i32) -> u64 {
        self.infoset_hash_tagged(bucket, tex_char, StackTag::from_stack_chips(stack_chips))
    }

    #[inline(always)]
    pub(crate) fn infoset_hash_tagged(
        &self,
        bucket: u8,
        tex_char: Option<char>,
        stack_tag: StackTag,
    ) -> u64 {
        let mut h = self.prefix_hash;
        h = fnv1a_combine(h, self.round);
        h = fnv1a_combine(h, b':');
        h = fnv1a_combine(h, bucket);
        if self.round == ROUND_PREFLOP {
            h = fnv1a_combine(h, 0);
        }
        h = fnv1a_combine(h, b'/');
        for &a in &self.cur_acts[..self.cur_len as usize] {
            h = fnv1a_combine(h, a);
        }
        if let Some(tc) = tex_char {
            h = mix_texture_hash(h, tc);
        }
        stack_tag.mix_hash(h)
    }

    #[cold]
    pub fn format_infoset_key(
        &self,
        pf_name: &str,
        bucket: u8,
        tex_char: Option<char>,
        stack_chips: i32,
        rich_history: bool,
    ) -> String {
        let stack_prefix = StackTag::from_stack_chips(stack_chips).prefix_str();
        let mut out = String::with_capacity(36);
        if self.round == ROUND_PREFLOP {
            out.push_str("P:");
            out.push_str(pf_name);
            out.push('/');
        } else if !rich_history || self.round == ROUND_RIVER {
            let round_code = match self.round {
                ROUND_FLOP => "F:B",
                ROUND_TURN => "T:B",
                ROUND_RIVER => "R:B",
                _ => "X:B",
            };
            out.push_str(round_code);
            if bucket < 10 {
                out.push('0');
            }
            let _ = write!(out, "{bucket}");
            out.push('/');
        } else {
            if self.pf_len > 0 {
                out.push_str("P:");
                push_unpacked_coarse(&mut out, self.pf_packed, self.pf_len);
                out.push('|');
            }
            if self.round >= ROUND_TURN && self.f_len > 0 {
                out.push_str("F:");
                push_unpacked_coarse(&mut out, self.f_packed, self.f_len);
                out.push('|');
            }
            let round_code = if self.round == ROUND_FLOP {
                "F:B"
            } else {
                "T:B"
            };
            out.push_str(round_code);
            if bucket < 10 {
                out.push('0');
            }
            let _ = write!(out, "{bucket}");
            out.push('/');
        }
        for &a in &self.cur_acts[..self.cur_len as usize] {
            out.push(action_to_char(a));
        }
        if let Some(tc) = tex_char {
            out = append_texture_to_key(&out, tc);
        }
        if stack_prefix.is_empty() {
            out
        } else {
            format!("{stack_prefix}{out}")
        }
    }

    #[inline(always)]
    pub fn step(
        &self,
        action: u8,
        stack_cap: i32,
        is_wet_flop: bool,
        rich_history: bool,
    ) -> StepResult {
        let curr = self.current_player as usize;
        if action == FOLD {
            return StepResult::FoldWin {
                loser_contrib: self.contributions[curr],
                loser: curr,
            };
        }

        let mut next = *self;
        let opp = 1 - curr;
        let pot = next.contributions[0] + next.contributions[1];

        match action {
            CALL_CHECK => {
                let diff = (next.contributions[opp] - next.contributions[curr]).max(0);
                next.contributions[curr] = (next.contributions[curr] + diff).min(stack_cap);
            }
            RAISE_MIN => {
                let diff = (next.contributions[opp] - next.contributions[curr]).max(0);
                let pot_with_call = pot + diff;
                let raise_amt = if next.round == ROUND_PREFLOP && next.raises_this_round == 0 {
                    PREFLOP_OPEN_MIN_TOTAL
                        .saturating_sub(next.contributions[curr] + diff)
                        .max(BIG_BLIND)
                } else if next.round == ROUND_PREFLOP {
                    RAISE_MIN_AMT
                } else if diff > 0 {
                    (pot_with_call / 3).max(RAISE_MIN_AMT)
                } else {
                    (pot_with_call / 3).max(BIG_BLIND)
                };
                next.contributions[curr] =
                    (next.contributions[curr] + diff + raise_amt).min(stack_cap);
                next.raises_this_round += 1;
            }
            RAISE_THIRD_POT => {
                let diff = (next.contributions[opp] - next.contributions[curr]).max(0);
                let pot_with_call = pot + diff;
                let raise_amt = if next.round == ROUND_PREFLOP && next.raises_this_round == 0 {
                    (pot_with_call / 3).max(RAISE_THIRD_POT_FLOOR)
                } else if next.round == ROUND_PREFLOP {
                    let target_total = next.contributions[opp] * 7 / 2;
                    target_total
                        .saturating_sub(next.contributions[curr] + diff)
                        .max(RAISE_THIRD_POT_FLOOR)
                } else if diff > 0 {
                    (pot_with_call * 3 / 5).max(RAISE_THIRD_POT_FLOOR)
                } else {
                    (pot_with_call * 3 / 5).max(BIG_BLIND * 2)
                };
                next.contributions[curr] =
                    (next.contributions[curr] + diff + raise_amt).min(stack_cap);
                next.raises_this_round += 1;
            }
            RAISE_HALF_POT => {
                let diff = (next.contributions[opp] - next.contributions[curr]).max(0);
                let is_wet = next.round == ROUND_FLOP && is_wet_flop;
                let pot_with_call = pot + diff;
                let raise_amt = if next.round == ROUND_RIVER {
                    (pot_with_call * 3 / 2).max(RAISE_HALF_POT_FLOOR_RIVER)
                } else if next.round == ROUND_TURN {
                    (pot_with_call * 3 / 2).max(RAISE_HALF_POT_FLOOR_TURN)
                } else if is_wet {
                    pot_with_call.max(RAISE_HALF_POT_FLOOR_WET)
                } else if next.round == ROUND_FLOP {
                    (pot_with_call * 3 / 4).max(RAISE_HALF_POT_FLOOR_DRY)
                } else {
                    (pot_with_call / 2).max(RAISE_HALF_POT_FLOOR_DRY)
                };
                next.contributions[curr] =
                    (next.contributions[curr] + diff + raise_amt).min(stack_cap);
                next.raises_this_round += 1;
            }
            ALL_IN => {
                next.contributions[curr] = stack_cap;
                next.raises_this_round += 1;
            }
            _ => unreachable!(),
        }

        let idx = next.cur_len as usize;
        if next.round == ROUND_PREFLOP {
            if idx < 4 && (1..=5).contains(&action) && next.pf_seq != u16::MAX {
                next.pf_seq = next.pf_seq * 5 + u16::from(action);
            } else {
                next.pf_seq = u16::MAX;
            }
        }
        if idx < MAX_ROUND_ACTIONS {
            next.cur_acts[idx] = action;
            next.cur_len += 1;
        }
        let n = next.cur_len as usize;
        let round_over = n >= 2 && {
            let last = next.cur_acts[n - 1];
            let prev = next.cur_acts[n - 2];
            last == CALL_CHECK && (prev == CALL_CHECK || prev >= RAISE_MIN || prev == FOLD)
        };

        if round_over {
            if (next.contributions[0] >= stack_cap && next.contributions[1] >= stack_cap)
                || next.round == ROUND_RIVER
            {
                return StepResult::Showdown {
                    contributions: next.contributions,
                };
            }

            if next.round == ROUND_PREFLOP {
                if rich_history && n > 0 {
                    next.pf_packed = pack_coarse_actions(&next.cur_acts[..n]);
                    next.pf_len = next.cur_len;
                    let mut h = FNV_OFFSET_BASIS;
                    h = fnv1a_combine(h, b'P');
                    for &a in &next.cur_acts[..n] {
                        let b = match a {
                            0 => b'f',
                            1 => b'c',
                            _ => b'r',
                        };
                        h = fnv1a_combine(h, b);
                    }
                    next.prefix_hash = fnv1a_combine(h, b'|');
                }
                next.round = ROUND_FLOP;
            } else if next.round == ROUND_FLOP {
                if rich_history && n > 0 {
                    next.f_packed = pack_coarse_actions(&next.cur_acts[..n]);
                    next.f_len = next.cur_len;
                    let mut h = next.prefix_hash;
                    h = fnv1a_combine(h, b'F');
                    for &a in &next.cur_acts[..n] {
                        let b = match a {
                            0 => b'f',
                            1 => b'c',
                            _ => b'r',
                        };
                        h = fnv1a_combine(h, b);
                    }
                    next.prefix_hash = fnv1a_combine(h, b'|');
                }
                next.round = ROUND_TURN;
            } else {
                next.prefix_hash = FNV_OFFSET_BASIS;
                next.round = ROUND_RIVER;
            }
            next.raises_this_round = 0;
            next.cur_len = 0;
            next.current_player = 1;
        } else {
            next.current_player = opp as u8;
        }

        StepResult::Continue(next)
    }
}
