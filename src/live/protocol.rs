/* Live Client Network Protocols, Payloads, and Action Mappings */
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::game::config::*;
use crate::game::holdem::{Card, Rank, Suit};

#[derive(Deserialize, Debug, Clone)]
pub struct GuestResponse {
    pub uuid: String,
    #[serde(default)]
    pub chips: i32,
}

#[derive(Serialize, Debug, Clone)]
pub struct JoinRoomPayload {
    pub uuid: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct ActPayload {
    pub player_id: String,
    pub action: String,
    pub amount: i32,
}

pub fn parse_json_i32(v: Option<&Value>) -> i32 {
    let Some(val) = v else {
        return 0;
    };
    if let Some(i) = val.as_i64() {
        i as i32
    } else if let Some(f) = val.as_f64() {
        f as i32
    } else if let Some(s) = val.as_str() {
        s.parse::<i32>().unwrap_or(0)
    } else {
        0
    }
}

/* Parse Card from server string format ("As", "Ah", "A♠", "10c", etc.) */
pub fn parse_card_str(s: &str) -> Option<Card> {
    let s = s.trim_matches('\'').trim();
    let mut chars = s.chars();
    let first = chars.next()?;
    let (rank, suit_char) = if first == '1' {
        let second = chars.next()?;
        if second != '0' {
            return None;
        }
        (Rank::Ten, chars.next()?)
    } else {
        let r = match first.to_ascii_uppercase() {
            '2' => Rank::Two,
            '3' => Rank::Three,
            '4' => Rank::Four,
            '5' => Rank::Five,
            '6' => Rank::Six,
            '7' => Rank::Seven,
            '8' => Rank::Eight,
            '9' => Rank::Nine,
            'T' => Rank::Ten,
            'J' => Rank::Jack,
            'Q' => Rank::Queen,
            'K' => Rank::King,
            'A' => Rank::Ace,
            _ => return None,
        };
        (r, chars.next()?)
    };

    let suit = match suit_char {
        'c' | 'C' | '♣' => Suit::Clubs,
        'd' | 'D' | '♦' => Suit::Diamonds,
        'h' | 'H' | '♥' => Suit::Hearts,
        's' | 'S' | '♠' => Suit::Spades,
        _ => return None,
    };

    Some(Card { rank, suit })
}

/* Map CFR Action Index (0: FOLD, 1: CALL/CHECK, 2: RAISE_MIN, 3: RAISE_THIRD_POT, 4: RAISE_HALF_POT, 5: ALL_IN) to server legal string */
pub fn map_action_index(
    idx: usize,
    legal: &[String],
    to_call: i32,
    pot: i32,
    is_wet: bool,
) -> Option<(String, i32)> {
    map_action_index_round(idx, legal, to_call, pot, is_wet, 2)
}

/* Street-aware CFR Action Index mapping with dynamic River/Turn bet sizing */
pub fn map_action_index_round(
    idx: usize,
    legal: &[String],
    to_call: i32,
    pot: i32,
    is_wet: bool,
    round: u8,
) -> Option<(String, i32)> {
    map_action_index_round_bb(idx, legal, to_call, pot, is_wet, round, BIG_BLIND)
}

/* Blind-level and Street-aware CFR Action Index mapping with dynamic River/Turn bet sizing */
pub fn map_action_index_round_bb(
    idx: usize,
    legal: &[String],
    to_call: i32,
    pot: i32,
    is_wet: bool,
    round: u8,
    table_bb: i32,
) -> Option<(String, i32)> {
    map_action_index_round_bb_chips(idx, legal, to_call, pot, is_wet, round, table_bb, 0)
}

/* Blind-level, Street-aware, and Stack-aware CFR Action Index mapping */
#[allow(clippy::too_many_arguments)]
pub fn map_action_index_round_bb_chips(
    idx: usize,
    legal: &[String],
    to_call: i32,
    pot: i32,
    is_wet: bool,
    round: u8,
    table_bb: i32,
    my_chips: i32,
) -> Option<(String, i32)> {
    map_action_index_full(
        idx, legal, to_call, pot, is_wet, round, table_bb, my_chips, 0,
    )
}

/* Full CFR Action Index mapping including explicit server `current_bet` when available */
#[allow(clippy::too_many_arguments)]
pub fn map_action_index_full(
    idx: usize,
    legal: &[String],
    to_call: i32,
    pot: i32,
    is_wet: bool,
    round: u8,
    table_bb: i32,
    my_chips: i32,
    current_bet: i32,
) -> Option<(String, i32)> {
    let bb = if table_bb > 0 { table_bb } else { BIG_BLIND };
    let ctx = ActionMapCtx {
        legal,
        to_call,
        pot,
        is_wet,
        round,
        bb,
        my_chips,
        current_bet,
    };

    match idx as u8 {
        FOLD => map_fold_action(&ctx),
        CALL_CHECK => map_call_check_action(&ctx),
        RAISE_MIN..=RAISE_HALF_POT => map_raise_action(idx as u8, &ctx),
        ALL_IN => map_allin_action(&ctx),
        _ => None,
    }
}

struct ActionMapCtx<'a> {
    legal: &'a [String],
    to_call: i32,
    pot: i32,
    is_wet: bool,
    round: u8,
    bb: i32,
    my_chips: i32,
    current_bet: i32,
}

impl ActionMapCtx<'_> {
    fn has(&self, a: &str) -> bool {
        self.legal.iter().any(|l| l.eq_ignore_ascii_case(a))
    }
}

fn map_fold_action(c: &ActionMapCtx<'_>) -> Option<(String, i32)> {
    if c.to_call == 0 {
        if c.has("check") {
            Some(("check".to_string(), 0))
        } else if c.has("fold") {
            Some(("fold".to_string(), 0))
        } else {
            None
        }
    } else if c.has("fold") {
        Some(("fold".to_string(), 0))
    } else if c.has("check") {
        Some(("check".to_string(), 0))
    } else {
        None
    }
}

fn map_call_check_action(c: &ActionMapCtx<'_>) -> Option<(String, i32)> {
    if c.to_call > 0 {
        if c.has("call") {
            Some(("call".to_string(), 0))
        } else if c.has("all_in") {
            /* Action Bridge: when calling puts bot all-in and server only lists "all_in" */
            Some(("all_in".to_string(), 0))
        } else if c.has("allin") {
            Some(("allin".to_string(), 0))
        } else if c.has("check") {
            Some(("check".to_string(), 0))
        } else {
            None
        }
    } else if c.has("check") {
        Some(("check".to_string(), 0))
    } else if c.has("call") {
        Some(("call".to_string(), 0))
    } else {
        None
    }
}

fn compute_half_pot_raise(
    c: &ActionMapCtx<'_>,
    is_unopened_or_limped_pf: bool,
    is_preflop_4bet_plus: bool,
    opp_bet_pf: i32,
    min_raise: i32,
    base_bet: i32,
) -> i32 {
    let bb = c.bb;
    let preflop_open_min_total = (bb * 5 / 2).max(bb * 2);
    if is_unopened_or_limped_pf {
        (bb * 7 / 2).max(preflop_open_min_total)
    } else if is_preflop_4bet_plus {
        (opp_bet_pf * 23 / 10).max(min_raise)
    } else if c.round == 1 && c.to_call > 0 {
        (opp_bet_pf * 3)
            .min(opp_bet_pf + c.pot)
            .max(opp_bet_pf * 11 / 4)
            .max(min_raise)
    } else {
        let s = if c.round == 4 {
            (c.pot * 5 / 4).max((bb * 5 / 2).max(bb))
        } else if c.round == 3 {
            (c.pot * 3 / 4).max((bb * 2).max(bb))
        } else if c.is_wet {
            (c.pot * 3 / 4).max((bb * 3 / 2).max(bb))
        } else {
            (c.pot / 2).max(bb)
        };
        if base_bet > 0 {
            (base_bet + s).max(min_raise)
        } else {
            s.max(min_raise)
        }
    }
}

fn compute_raise_amount(idx: u8, c: &ActionMapCtx<'_>) -> (i32, i32) {
    let bb = c.bb;
    let preflop_open_min_total = (bb * 5 / 2).max(bb * 2);

    let is_unopened_or_limped_pf = c.round == 1
        && if c.current_bet > 0 {
            c.current_bet <= bb
        } else {
            c.to_call <= bb && c.pot <= bb * 2 + bb / 2
        };
    let opp_bet_pf = if c.current_bet > 0 {
        c.current_bet.max(c.to_call)
    } else {
        ((c.pot + c.to_call) / 2).max(c.to_call + bb / 2)
    };
    let min_raise = if c.round == 1 {
        if is_unopened_or_limped_pf {
            preflop_open_min_total
        } else if c.to_call > 0 {
            (opp_bet_pf + c.to_call.max(bb)).max(preflop_open_min_total)
        } else {
            (bb * 2).max(preflop_open_min_total)
        }
    } else if c.to_call > 0 {
        (opp_bet_pf + c.to_call.max(bb)).max(c.to_call * 2)
    } else {
        bb
    };
    let base_bet = if c.round == 1 && c.to_call == 0 {
        bb
    } else if c.current_bet > 0 {
        c.current_bet.max(c.to_call)
    } else {
        c.to_call
    };
    let my_posted_bet = c.current_bet.saturating_sub(c.to_call);
    let is_preflop_4bet_plus = c.round == 1 && !is_unopened_or_limped_pf && opp_bet_pf > bb * 5;

    let amt = match idx {
        RAISE_THIRD_POT => {
            if is_unopened_or_limped_pf {
                (bb * 3).max(preflop_open_min_total)
            } else if is_preflop_4bet_plus {
                (opp_bet_pf * 11 / 5).max(min_raise)
            } else if c.round == 1 && c.to_call > 0 {
                (opp_bet_pf * 11 / 4).max(min_raise)
            } else {
                let s = (c.pot / 3).max(bb);
                if base_bet > 0 {
                    (base_bet + s).max(min_raise)
                } else {
                    s.max(min_raise)
                }
            }
        }
        RAISE_HALF_POT => compute_half_pot_raise(
            c,
            is_unopened_or_limped_pf,
            is_preflop_4bet_plus,
            opp_bet_pf,
            min_raise,
            base_bet,
        ),
        _ => {
            if is_unopened_or_limped_pf {
                preflop_open_min_total
            } else if is_preflop_4bet_plus {
                (opp_bet_pf * 21 / 10).max(min_raise)
            } else if c.round == 1 && c.to_call > 0 {
                (opp_bet_pf * 5 / 2).max(min_raise)
            } else if c.round == 1 {
                (base_bet + bb).max(min_raise)
            } else if c.to_call > 0 {
                let s = ((c.pot + c.to_call) / 4).max(bb);
                (c.to_call + s).max(min_raise)
            } else {
                (c.pot / 4).max(min_raise)
            }
        }
    };
    (amt, my_posted_bet)
}

fn map_raise_action(idx: u8, c: &ActionMapCtx<'_>) -> Option<(String, i32)> {
    let (amt, my_posted_bet) = compute_raise_amount(idx, c);
    if c.round == 1
        && c.my_chips > 0
        && c.to_call > 0
        && (amt * 20 >= c.my_chips * 11 || c.my_chips - amt <= c.bb * 3)
    {
        /* If a preflop 3-bet/4-bet commits >= 55% of remaining stack or leaves <= 3 BB behind, jam all-in! */
        if c.has("all_in") {
            return Some(("all_in".to_string(), 0));
        } else if c.has("allin") {
            return Some(("allin".to_string(), 0));
        } else if c.has("raise") {
            return Some(("raise".to_string(), c.my_chips + my_posted_bet));
        }
    }
    if c.has("raise") {
        Some(("raise".to_string(), amt))
    } else if c.has("bet") {
        Some(("bet".to_string(), amt))
    } else if c.has("all_in") {
        Some(("all_in".to_string(), 0))
    } else if c.has("allin") {
        Some(("allin".to_string(), 0))
    } else if c.to_call > 0 && c.has("call") {
        Some(("call".to_string(), 0))
    } else if c.has("check") {
        Some(("check".to_string(), 0))
    } else {
        None
    }
}

fn map_allin_action(c: &ActionMapCtx<'_>) -> Option<(String, i32)> {
    let my_posted_bet = c.current_bet.saturating_sub(c.to_call);
    let allin_amt = if c.my_chips > 0 {
        c.my_chips + my_posted_bet
    } else {
        (c.pot * 2).max(c.bb * 2)
    };
    if c.to_call > 0 && c.my_chips > 0 && c.to_call >= c.my_chips && c.has("call") {
        Some(("call".to_string(), 0))
    } else if c.has("all_in") {
        Some(("all_in".to_string(), 0))
    } else if c.has("allin") {
        Some(("allin".to_string(), 0))
    } else if c.has("raise") {
        Some(("raise".to_string(), allin_amt))
    } else if c.has("bet") {
        Some(("bet".to_string(), allin_amt))
    } else if c.to_call > 0 && c.has("call") {
        Some(("call".to_string(), 0))
    } else if c.has("check") {
        Some(("check".to_string(), 0))
    } else {
        None
    }
}

/* Pseudo-Harmonic mapping between two discrete bet amounts s1 <= s2 for actual bet x:
 * Returns probability of mapping to s1 (lower action) preserving pot odds and EV.
 * Ref: Ganzfried & Sandholm (2013). */
#[inline]
pub fn pseudo_harmonic_prob(x: i32, s1: i32, s2: i32, pot: i32) -> f64 {
    if s2 <= s1 || x <= s1 {
        return 1.0;
    }
    if x >= s2 {
        return 0.0;
    }
    let x_f = x as f64;
    let s1_f = s1 as f64;
    let s2_f = s2 as f64;
    let pot_f = pot.max(1) as f64;

    let w1 = ((s2_f - x_f) / (s2_f - s1_f)) * ((s1_f + pot_f) / (x_f + pot_f));
    w1.clamp(0.0, 1.0)
}

/* Pseudo-Harmonic action translation: maps an arbitrary continuous bet amount into
 * a discrete action index (2: RAISE_MIN, 3: RAISE_THIRD_POT, 4: RAISE_HALF_POT, 5: ALL_IN)
 * preserving pot odds and expectation value.
 * Ref: Ganzfried & Sandholm (2013). */
pub fn pseudo_harmonic_action_mapping<R: rand::Rng>(
    bet_amt: i32,
    pot: i32,
    allin_threshold: i32,
    rng: &mut R,
) -> u8 {
    if bet_amt >= allin_threshold {
        return ALL_IN;
    }

    let eff_bb = (allin_threshold / 35).max(1);
    let s_min = eff_bb;
    let s_third = (pot / 3).max(eff_bb);
    let s_half = (pot / 2).max(eff_bb);

    if bet_amt <= s_min {
        return RAISE_MIN;
    }

    if bet_amt < s_third {
        let p_min = pseudo_harmonic_prob(bet_amt, s_min, s_third, pot);
        if rng.gen::<f64>() <= p_min {
            RAISE_MIN
        } else {
            RAISE_THIRD_POT
        }
    } else if bet_amt < s_half {
        let p_third = pseudo_harmonic_prob(bet_amt, s_third, s_half, pot);
        if rng.gen::<f64>() <= p_third {
            RAISE_THIRD_POT
        } else {
            RAISE_HALF_POT
        }
    } else {
        let s_overbet = (pot * 3 / 2).max(s_half + eff_bb);
        if bet_amt <= s_overbet {
            RAISE_HALF_POT
        } else {
            let s_cap = allin_threshold.max(s_overbet + eff_bb);
            let p_half = pseudo_harmonic_prob(bet_amt, s_overbet, s_cap, pot);
            if rng.gen::<f64>() <= p_half {
                RAISE_HALF_POT
            } else {
                ALL_IN
            }
        }
    }
}
