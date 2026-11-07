/* Preflop & Postflop Tactical Guards (All-In Defense, Pot Control, Draw Odds, Steals & Blockers) */
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::{
    detect_board_texture, detect_river_nut_blocker, is_dangerous_board, is_four_flush_board,
    is_four_straight_board, preflop_strength_tier,
};
use crate::game::config::*;
use crate::game::holdem::Card;

impl OpponentTracker {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_preflop_and_postflop_guards(
        &self,
        probs: &mut [f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        pot: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        active_opponents: usize,
        bb: i32,
        eff_allin_threshold: i32,
        pot_odds: f64,
        is_three_flush: bool,
    ) {
        let is_preflop = round == 1;
        let can_raise = legal_actions
            .iter()
            .any(|&a| (RAISE_MIN..=ALL_IN).contains(&a));
        let is_passive_station = self.is_passive_station();

        /* 1. Pot-Odds Aware All-In Defense and Heavy Bet Guard:
         * Never call off stack with trash hands to massive bets due to mixed-strategy noise!
         * Conversely:
         * - If to_call is tiny or pot odds are overwhelming (pot_odds <= 0.15), NEVER fold (pot-committed)!
         * - Strong draws (OESD, Flush Draws, Combo Draws) on Flop/Turn have direct equity and must not fold to reasonable bets.
         * - Monster hands (Tier 1 & Tier 2: AA-99, AK, AQ) must never fold preflop to normal shoves!
         */
        let is_four_flush_board_now = board.map(is_four_flush_board).unwrap_or(false);
        let never_fold_postflop = !is_preflop
            && (bucket >= 83 || (bucket == 82 && !is_four_flush_board_now && to_call < pot * 2));
        if to_call > 0
            && (pot_odds <= 0.12
                || (pot_odds <= 0.15 && (is_preflop || bucket >= 15))
                || never_fold_postflop)
        {
            if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                let f_prob = probs[f_i];
                probs[f_i] = 0.0;
                if let Some(c_i) = legal_actions
                    .iter()
                    .position(|&a| a == CALL_CHECK)
                    .or_else(|| legal_actions.iter().position(|&a| a == ALL_IN))
                {
                    probs[c_i] += f_prob.max(0.50);
                }
            }
        }

        /* Preflop Trash, Priced-In BB Defense & Limp-Pot OOP ISO-Raise Guard:
         * 1) In BB facing a limp (to_call == 0): never iso-raise OOP with Tier 6..=8 hands (e.g. 53s, J3o); take the free flop!
         * 2) Facing a bet with Tier 7..=8 hands (e.g. 54o, 72o): never 3x open-raise or 3-bet bluff!
         * 3) Priced-In BB Protection: if pot_odds <= 0.26 (e.g. BB already posted facing a min-raise
         *    or short-stack shove), do NOT force-fold playable/borderline hands! */
        if is_preflop {
            let pf_tier = preflop_strength_tier(bucket);
            if to_call == 0 && pf_tier >= 6 {
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] = (probs[c_i] + shifted).max(0.95);
                }
            } else if to_call > 0 && pf_tier >= 7 {
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if pot_odds <= 0.22 || (pot_odds <= 0.26 && pf_tier <= 7) {
                    /* Priced in from the Big Blind: shift removed raise weight into calling rather than folding */
                    if let Some(c_i) = legal_actions
                        .iter()
                        .position(|&a| a == CALL_CHECK)
                        .or_else(|| legal_actions.iter().position(|&a| a == ALL_IN))
                    {
                        probs[c_i] += shifted;
                    }
                } else if to_call > bb / 2 && (pf_tier == 8 || pot_odds > 0.28) {
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                            shifted += probs[c_i];
                            probs[c_i] = 0.0;
                        }
                        probs[f_i] = (probs[f_i] + shifted).max(0.98);
                    }
                } else if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                    probs[f_i] += shifted;
                }
            } else if to_call > bb / 2 && (pf_tier >= 5 || bucket >= 92) {
                /* Facing a preflop raise with Tier 5..=6 or offsuit Tier 4 (ATo, A9o, KJo — Hands 1 & 5):
                 * 1) Do not 3-bet / 4-bet bloat the pot with speculative or dominated offsuit hands!
                 * 2) Facing a 3-bet or large raise (to_call >= 3 BB, pot_odds > 0.26) with Tier 6 (e.g. T8o in b1a Hand 5)
                 *    or offsuit Tier 5: fold instead of calling off 25-30% of stack preflop! */
                let is_extreme_shove_bot =
                    self.total_hands >= 15 && self.raise_ratio() > 0.50 && self.vpip_ratio() > 0.70;
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if to_call >= bb * 3
                    && pot_odds > 0.26
                    && !is_extreme_shove_bot
                    && (pf_tier >= 6 || (pf_tier == 5 && bucket >= 91))
                {
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                            shifted += probs[c_i];
                            probs[c_i] = 0.0;
                        }
                        probs[f_i] = (probs[f_i] + shifted).max(0.98);
                    }
                } else if to_call < bb * 3 && pot_odds <= 0.36 {
                    if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                        probs[c_i] += shifted;
                    }
                } else if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                    probs[f_i] += shifted;
                }
            }
            if to_call <= bb * 4 && bucket >= 6 {
                /* Facing a standard preflop open/limp (to_call <= 4 BB) with hands below TT/AK/AQs (bucket >= 6, e.g. 77):
                 * Never 3-bet jam ALL_IN directly for full stacks; shift ALL_IN weight into standard 3-bet or flat call! */
                if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                    let s = probs[ai_i];
                    probs[ai_i] = 0.0;
                    if let Some(t_i) = legal_actions
                        .iter()
                        .position(|&a| a == RAISE_THIRD_POT)
                        .or_else(|| legal_actions.iter().position(|&a| a == RAISE_MIN))
                        .or_else(|| legal_actions.iter().position(|&a| a == CALL_CHECK))
                    {
                        probs[t_i] += s;
                    }
                }
            }
        }

        /* A situation is a dangerous shove/heavy bet if to_call is substantial (>= 4 BB) and pot odds are meaningful (> 0.18).
         * On the River, require to_call to be either an actual overbet/heavy bet relative to the pot (pot_odds >= 0.30, >= 4 BB)
         * or an all-in shove (>= ALLIN_GUARD_THRESHOLD or !can_raise). */
        let is_substantial_bet = to_call >= bb * 4;
        let is_shove_faced = is_substantial_bet
            && pot_odds > 0.18
            && (to_call >= eff_allin_threshold
                || !can_raise
                || (round == 1 && to_call >= bb * 10)
                || (round == 4 && to_call >= bb * 5 && pot_odds >= 0.30)
                || (round == 3 && to_call >= bb * 15 && pot_odds >= 0.33)
                || (round == 2 && to_call >= bb * 20));

        if is_shove_faced {
            let is_known_maniac = self.total_actions() >= 8
                && (self.raise_ratio() > 0.35
                    || self.pfr_ratio() > 0.40
                    || self.vpip_ratio() > 0.65);
            let is_extreme_shove_bot =
                self.total_hands >= 15 && self.raise_ratio() > 0.50 && self.vpip_ratio() > 0.70;

            if is_preflop {
                let pf_tier = preflop_strength_tier(bucket);
                if to_call >= bb * 40 {
                    /* Facing massive cold shove (>= 40 BB, Match 3): AA and KK (bucket <= 1) never fold;
                     * against a known maniac, QQ and AK (pf_tier == 1) also never fold. */
                    if bucket <= 1 || (is_known_maniac && pf_tier == 1) {
                        if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                            probs[f_i] = 0.0;
                        }
                    }
                } else if (pf_tier <= 1
                    || (is_known_maniac && pf_tier <= 3 && to_call < eff_allin_threshold)
                    || (is_extreme_shove_bot
                        && pf_tier <= 6
                        && pot_odds <= 0.46
                        && to_call < bb * 18))
                    && to_call < eff_allin_threshold * 2
                {
                    /* Tier 1 (or Tiers 1..=6 vs extreme shove-bot at < 18 BB and <= 46% pot odds, b1.md Item E) never fold preflop */
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        let f_w = probs[f_i];
                        probs[f_i] = 0.0;
                        if let Some(c_i) = legal_actions
                            .iter()
                            .position(|&a| a == CALL_CHECK)
                            .or_else(|| legal_actions.iter().position(|&a| a == ALL_IN))
                        {
                            probs[c_i] += f_w;
                        }
                    }
                }
            }

            let is_trash = if is_preflop {
                let pf_tier = preflop_strength_tier(bucket);
                if pot_odds <= 0.22 || (pot_odds <= 0.26 && pf_tier <= 7) {
                    /* Priced in from BB against a short-stack shove: never auto-fold! */
                    false
                } else if to_call >= bb * 40 {
                    /* Facing massive cold shove (>= 40 BB, Match 3):
                     * - vs known maniac: call with Tier 1 (AA, KK, QQ, AK)
                     * - vs tight/unknown in multi-way: only AA, KK (bucket <= 1)
                     * - vs tight/unknown in HU: AA, KK, QQ, AKs (bucket <= 2 || bucket == 13) */
                    if is_known_maniac {
                        pf_tier > 1
                    } else if active_opponents >= 2 {
                        bucket > 1
                    } else {
                        !(bucket <= 2 || bucket == 13)
                    }
                } else if to_call >= eff_allin_threshold {
                    /* Facing 35-39 BB shove:
                     * - vs known maniac: call with Tiers 1-2 (AA-99, AK, AQ, AJs, KQs)
                     * - vs tight/unknown in HU: Tier 1 + JJ, AQs (bucket == 3 || bucket == 14)
                     * - vs tight/unknown in multi-way: Tier 1 only */
                    if is_known_maniac {
                        pf_tier > 2
                    } else if active_opponents == 1 {
                        pf_tier > 1 && bucket != 3 && bucket != 14
                    } else {
                        pf_tier > 1
                    }
                } else if to_call >= bb * 18 {
                    /* Facing deep 18-34 BB shove (e.g. 29 BB shove with QJo):
                     * - vs known maniac / extreme shove-bot: call with Tiers 1..=3 or pocket pairs (bucket <= 11);
                     *   fold dominated offsuit Tier 4 (QJo, KJo, ATo) and Tiers 5..=8!
                     * - vs tight/unknown: call with Tiers 1..=2 or pocket pairs 88-77 (bucket <= 7) */
                    if is_known_maniac || is_extreme_shove_bot {
                        pf_tier > 3 && bucket > 11
                    } else {
                        pf_tier > 2 && bucket > 7
                    }
                } else if is_extreme_shove_bot && pot_odds <= 0.46 {
                    /* Facing < 18 BB shove from proven shove-bot (b1.md Item E, Hand 6): call with Tiers 1..=6 */
                    pf_tier > 6
                } else if is_known_maniac {
                    /* Facing < 18 BB shove from known maniac: call with Tiers 1..=5 */
                    pf_tier > 5
                } else {
                    /* Facing standard < 18 BB shove: Tiers 5..=8 fold */
                    pf_tier >= 5
                }
            } else if round == 4 {
                /* On River facing massive overbet/shove: only High Overpair (54+) or Genuine Two Pair+ (57+) can call;
                 * on a 4-flush board, baby/board flushes (bucket <= 79) and non-flushes must also fold!
                 * on a 4-straight board (e.g. K-A-2-Q-J with a set, bucket 68), hands below Straight (bucket < 73) must fold! */
                let is_four_flush = board.map(is_four_flush_board).unwrap_or(false);
                let is_four_straight = board.map(is_four_straight_board).unwrap_or(false);
                bucket < 54 || (is_four_flush && bucket <= 79) || (is_four_straight && bucket < 73)
            } else {
                /* Flop/Turn facing massive shove:
                 * On Flop (2 cards to come), strong draws have 32-54% equity and can call <= 0.35 pot odds.
                 * On Turn (1 card to come), only Combo Draws (24..=27) with <= 0.31 pot odds or Nut FD (22..=23) with <= 0.22 can call! */
                let is_wet = board.map(is_dangerous_board).unwrap_or(false);
                let is_paired = board
                    .map(|b| detect_board_texture(b).is_paired)
                    .unwrap_or(false);
                let is_strong_draw = (15..=18).contains(&bucket) || (19..=27).contains(&bucket);
                let draw_can_call_shove = if round == 3 {
                    (bucket >= 24 && pot_odds <= 0.31 && !is_three_flush)
                        || ((bucket == 26 || bucket == 27) && pot_odds <= 0.31)
                        || ((bucket == 22 || bucket == 23) && pot_odds <= 0.22)
                } else {
                    (is_strong_draw && pot_odds <= 0.35 && !is_three_flush)
                        || ((bucket == 22 || bucket == 23 || bucket >= 25) && pot_odds <= 0.35)
                };

                if draw_can_call_shove {
                    false
                } else if is_three_flush && bucket <= 55 {
                    /* Bare one pair or overpair without a made flush on a 3-flush board facing a massive shove */
                    true
                } else if round == 3 && is_paired && bucket < 57 {
                    /* Turn on paired board facing massive shove: no Genuine Two Pair / Trips / Set / FH (bucket < 57, Match 7) is trash */
                    true
                } else if bucket < 46 && !is_strong_draw {
                    true
                } else if round == 3 && (46..=55).contains(&bucket) && is_wet {
                    /* Bare one pair or overpair on a 4-to-straight or monotone turn is trash against a massive shove */
                    true
                } else {
                    false
                }
            };

            if is_trash && legal_actions.contains(&FOLD) {
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a == FOLD {
                        probs[i] = 1.0;
                    } else {
                        probs[i] = 0.0;
                    }
                }
            }
        }

        /* 1b. Postflop Bluff-Spew, Gutshot/Turn-Draw Overbet, and Turn/River Pot-Control Guard:
         * - Prevent suicidal all-in bluffs on ALL postflop streets (round >= 2) with hands below Trips/Set (bucket < 65),
         *   except Flop monster combo draws (25..=27).
         * - Suppress large overbets (RAISE_HALF_POT) on Turn/River with draws or air (bucket < 28) in medium/large pots (Match 10).
         * - On a 3-flush board without a flush, unmade draws (bucket < 28) must never lead-raise; take the free card!
         * - Turn facing a bet: never probe-raise with weak or marginal hands (bucket < 61, Match 7).
         * - Fold bare gutshots / pure air (bucket < 15) facing >= 33% pot bets (pot_odds >= 0.25, to_call >= 2 BB).
         * - Enforce strict single-card direct equity pot-odds on Turn draws (15..=27).
         * - Suppress turn/river lead donk-bets in bloated pots with multi-overcard underpairs/weak pairs (bucket 28..=38, Match 5). */
        if !is_preflop && bucket < 65 && !(round == 2 && (25..=27).contains(&bucket)) {
            /* All postflop streets: block all-in pushes for air, draws (except flop monster combo draws),
             * one-pair, overpair, and two-pair hands (< 65, Match 9 & Item I) */
            if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                probs[ai_i] = 0.0;
            }
        }

        if !is_preflop && (bucket < 15 || bucket == 28 || bucket == 29) {
            if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                probs[ai_i] = 0.0;
            }
            if to_call > 0 && (bucket < 15 || bucket == 28) {
                /* Facing a bet with pure air, gutshot, or crushed 4-flush/4-straight pair: never bluff-raise */
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        probs[i] = 0.0;
                    }
                }
                /* Strict Odds Guard (Match 4, Item E & b1.md Item C):
                 * - Facing >= 33% pot bet (pot_odds >= 0.25, >= 2 BB) on Flop/Turn with bucket < 15 or 28 -> 100% fold!
                 * - Facing >= 16% pot odds on Turn with a bare gutshot/air (bucket < 15) -> 100% fold!
                 * - Pure air without even a gutshot (bucket < 11 or 28, e.g. KJo on 7-7-2 in Hand 1) facing any bet >= 1 BB
                 *   with pot_odds > 0.12 on Flop/Turn -> 100% fold! */
                let overpriced_gutshot_or_air =
                    (round < 4 && to_call >= bb * 2 && pot_odds >= 0.25)
                        || (round == 3 && to_call >= bb && pot_odds >= 0.16 && bucket < 15)
                        || (round < 4
                            && to_call >= bb
                            && pot_odds > 0.12
                            && (bucket < 11 || bucket == 28));
                if overpriced_gutshot_or_air {
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        for p in probs.iter_mut() {
                            *p = 0.0;
                        }
                        probs[f_i] = 1.0;
                    }
                }
            }
        }

        /* Turn Draw Direct Pot-Odds Guard (Item J):
         * On the Turn (round == 3), only 1 card remains: OESD (15..=18) has ~17.4% equity,
         * Flush Draw (19..=23) has ~19.6% equity, and Combo Draw (24..=27) has ~26-32% equity. */
        if round == 3 && to_call >= bb * 2 && (15..=27).contains(&bucket) {
            let max_turn_draw_odds = if bucket >= 24 {
                0.31
            } else if bucket >= 19 {
                0.22
            } else {
                0.19
            };
            if pot_odds > max_turn_draw_odds {
                if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                    for p in probs.iter_mut() {
                        *p = 0.0;
                    }
                    probs[f_i] = 1.0;
                }
            }
        }

        if round == 2
            && to_call == 0
            && ((bucket < 15 && pot >= bb * 8) || ((28..=37).contains(&bucket) && pot >= bb * 10))
        {
            /* Bloated / 3-bet / 4-bet Flop pot (pot >= 8 BB, b1.md Item B & b1a Hands 2 & 5):
             * With pure air / bare gutshot (bucket < 15, e.g. KQh on 8-T-9 in 4-bet pot) or weak/middle pair
             * without draw in a 3-bet pot (28..=37, e.g. T8o on A-6-T), check for pot control instead of
             * donk-betting or bloating the pot! */
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        }

        let is_four_board_danger = board
            .map(|b| is_four_straight_board(b) || is_four_flush_board(b))
            .unwrap_or(false);
        if round >= 3 && to_call == 0 && is_four_board_danger && bucket < 73 {
            /* Turn/River on a 4-to-a-straight or 4-to-a-flush board (e.g. KQh on 8d-Ts-9c-Qc, or QQ set on K-A-2-Q-J):
             * Any unimproved One Pair, Overpair, Two Pair, or Set (bucket < 73) is strictly a bluff-catcher
             * and must check for pot control rather than jamming into made straights/flushes! */
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        } else if round >= 3 && to_call == 0 && bucket < 28 {
            /* Large turn/river bluffs with bare draws / air:
             * On 3-flush boards or pots >= 6 BB, block RAISE_HALF_POT and shift all raises into CALL_CHECK (Match 10) */
            if let Some(hp_i) = legal_actions.iter().position(|&a| a == RAISE_HALF_POT) {
                probs[hp_i] = 0.0;
            }
            if pot >= bb * 6 || is_three_flush {
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] = (probs[c_i] + shifted).max(0.95);
                }
            }
        } else if round >= 3
            && to_call == 0
            && pot >= bb * 8
            && (28..=38).contains(&bucket)
            && !(is_passive_station && bucket >= 35 && !is_three_flush)
        {
            /* Underpair under 2+ overcards or weak middle/bottom pair in a large Turn/River pot (Match 5):
             * Check for pot control instead of turning showdown value into a burning lead-bluff! */
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        }

        /* Pair + Strong Draw Defense on Flop & Turn (b1a Hand 4: 3c 3s on 2s-2c-5h-4c, bucket 33 or 38):
         * A made pair + OESD/Flush Draw has 10-14 outs + pair equity and must not overfold to standard bets (pot_odds <= 0.35)! */
        if round < 4
            && to_call > 0
            && (bucket == 33 || bucket == 38)
            && pot_odds <= 0.35
            && !is_three_flush
        {
            if let (Some(f_i), Some(c_i)) = (
                legal_actions.iter().position(|&a| a == FOLD),
                legal_actions.iter().position(|&a| a == CALL_CHECK),
            ) {
                let shift = probs[f_i] * 0.85;
                probs[f_i] -= shift;
                probs[c_i] = (probs[c_i] + shift).max(0.80);
            }
        }

        /* Turn facing a bet: never probe-raise or check-raise with weak or marginal hands (bucket < 61, Match 7) */
        if round == 3 && to_call > 0 && bucket < 61 {
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    probs[i] = 0.0;
                }
            }
        }

        /* 2. Preflop Button Steal Exploitation (Item K):
         * In Heads-Up (active_opponents == 1), Button opens ~80% of hands profitably (tiers 1..=6).
         * Shifts both FOLD and passive open-limp (CALL_CHECK) weight into RAISE_MIN / RAISE_THIRD_POT!
         */
        let max_steal_tier = if active_opponents == 1 {
            6
        } else if active_opponents == 2 {
            4
        } else {
            3
        };
        let steal_max_call = bb / 2;
        let pf_tier_now = preflop_strength_tier(bucket);
        if is_preflop && to_call <= steal_max_call && pf_tier_now <= max_steal_tier {
            let fold_idx = legal_actions.iter().position(|&a| a == FOLD);
            let call_idx = legal_actions.iter().position(|&a| a == CALL_CHECK);
            let raise_idx = legal_actions
                .iter()
                .position(|&a| a == RAISE_MIN || a == RAISE_THIRD_POT);
            if let Some(r_i) = raise_idx {
                if let Some(f_i) = fold_idx {
                    if probs[f_i] > 0.05 {
                        probs[r_i] += probs[f_i] * 0.85;
                        probs[f_i] *= 0.15;
                    }
                }
                if to_call > 0 {
                    if let Some(c_i) = call_idx {
                        let limp_shift = if pf_tier_now <= 4 { 0.75 } else { 0.55 };
                        let shifted = probs[c_i] * limp_shift;
                        probs[r_i] += shifted;
                        probs[c_i] -= shifted;
                    }
                }
            }
        }

        /* 3. Blocker-aware River Bluffing (Heads-Up only):
         * On the River (round == 4) when not facing a bet (to_call == 0) with weak hands (bucket < 28):
         * If we hold the Nut Blocker (Ace of flush suit or Broadway straight blocker),
         * boost aggressive bluffing on RAISE_HALF_POT (125% overbet) or RAISE_THIRD_POT.
         */
        if round == 4 && active_opponents == 1 && to_call == 0 && bucket < 28 {
            if let (Some(h), Some(b)) = (hole, board) {
                if detect_river_nut_blocker(h, b) {
                    let raise_overbet = legal_actions.iter().position(|&a| a == RAISE_HALF_POT);
                    let raise_third = legal_actions.iter().position(|&a| a == RAISE_THIRD_POT);
                    let check_idx = legal_actions.iter().position(|&a| a == CALL_CHECK);
                    let fold_idx = legal_actions.iter().position(|&a| a == FOLD);

                    if let Some(r_i) = raise_overbet.or(raise_third) {
                        probs[r_i] = (probs[r_i] * 1.50).max(0.20);
                        if let Some(c_i) = check_idx {
                            probs[c_i] *= 0.65;
                        }
                        if let Some(f_i) = fold_idx {
                            probs[f_i] *= 0.50;
                        }
                    }
                }
            }
        }

        /* 4. Idiot-end / Board Straight and River Overbet safety:
         * - If we have bucket 73 (Board Straight or Idiot-End straight on 4-to-straight board),
         *   NEVER blast a 125% pot overbet or shove into a checked pot!
         * - With a one-pair, overpair, or vulnerable two-pair hand on River (28 <= bucket < 61), suppress 125% pot overbets
         *   (`RAISE_HALF_POT` on river) and all-in shoves so we don't value-own ourselves for stacks (b1.md Item D, Hand 4).
         * - On a 3-flush or dangerous board on Turn/River, if we hold less than a Flush (bucket < 79)
         *   and are facing a substantial bet (pot_odds > 0.25), do not re-raise / jam over the top! */
        if round == 4 && bucket == 73 {
            if let Some(r_hp) = legal_actions.iter().position(|&a| a == RAISE_HALF_POT) {
                probs[r_hp] *= 0.10;
            }
            if let Some(r_ai) = legal_actions.iter().position(|&a| a == ALL_IN) {
                probs[r_ai] *= 0.05;
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = probs[c_i].max(0.50);
            }
        } else if round == 4
            && (74..=78).contains(&bucket)
            && (is_three_flush
                || board
                    .map(|b| detect_board_texture(b).is_paired || is_four_flush_board(b))
                    .unwrap_or(false))
        {
            /* Straight on a paired or 3/4-flush River board (e.g. 7c 6s on 5h-9s-7d-9h-8h):
             * Vulnerable to Full Houses and Flushes -> NEVER blast a 125% pot overbet (`RAISE_HALF_POT`) or `ALL_IN` shove,
             * and never raise facing a River bet! Shift weight into thin value (`RAISE_THIRD_POT`) or `CALL_CHECK`. */
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a == RAISE_HALF_POT || a == ALL_IN || (a >= RAISE_MIN && to_call > 0) {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if to_call == 0 && !is_three_flush && !board.map(is_four_flush_board).unwrap_or(false) {
                if let Some(r_t) = legal_actions.iter().position(|&a| a == RAISE_THIRD_POT) {
                    probs[r_t] = (probs[r_t] + shifted * 0.65).max(0.55);
                }
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] += shifted * 0.35;
                }
            } else if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.65);
            }
        } else if round == 4 && (28..61).contains(&bucket) {
            let is_three_straight = board
                .map(|b| {
                    let mut ranks = [false; 13];
                    for c in b {
                        ranks[c.rank as usize] = true;
                    }
                    (0..=9).any(|s| (s..s + 4).filter(|&r| ranks[r]).count() >= 3)
                })
                .unwrap_or(false);
            let is_dangerous = board.map(is_dangerous_board).unwrap_or(false)
                || is_three_flush
                || is_three_straight;
            if to_call > 0 {
                /* Facing a River bet with One Pair, Overpair, or weak Two Pair (bucket < 61, e.g. Match 2 with 86o on 5-8-4-3-4):
                 * NEVER raise and pot-commit into a 3-bet shove! Shift all raise weight into flat calling. */
                let mut shifted_raise = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted_raise += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if shifted_raise > 0.0 {
                    if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                        probs[c_i] += shifted_raise;
                    }
                }
            } else if is_passive_station && !is_dangerous && bucket >= 45 {
                /* Against a proven passive calling station on a safe board with Top Pair Good+ / Overpair / Two Pair (45..61):
                 * Do NOT suppress RAISE_HALF_POT! Only suppress ALL_IN and shift into RAISE_HALF_POT / RAISE_THIRD_POT. */
                let mut shifted = 0.0f64;
                if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                    shifted += probs[ai_i];
                    probs[ai_i] = 0.0;
                }
                if let Some(r_hp) = legal_actions.iter().position(|&a| a == RAISE_HALF_POT) {
                    probs[r_hp] += shifted * 0.70;
                } else if let Some(r_t) = legal_actions.iter().position(|&a| a == RAISE_THIRD_POT) {
                    probs[r_t] += shifted;
                }
            } else {
                /* River with 28..61 (One Pair, Overpair, or Bottom/Middle Two Pair, b1.md Item D, Hand 4):
                 * NEVER fire a 125% pot overbet (`RAISE_HALF_POT`) or `ALL_IN`!
                 * Shift overbet weight into thin 33% value (`RAISE_THIRD_POT`) on safe boards or `CALL_CHECK` on dangerous/3-straight boards. */
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a == RAISE_HALF_POT || a == ALL_IN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if is_dangerous {
                    if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                        probs[c_i] = (probs[c_i] + shifted).max(0.60);
                    }
                } else if bucket >= 46 {
                    if let Some(r_t) = legal_actions.iter().position(|&a| a == RAISE_THIRD_POT) {
                        probs[r_t] += shifted * 0.60;
                    }
                    if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                        probs[c_i] += shifted * 0.40;
                    }
                } else if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] += shifted;
                }
            }
        } else if round >= 3
            && (79..=81).contains(&bucket)
            && (board.map(is_four_flush_board).unwrap_or(false)
                || (is_three_flush
                    && board
                        .map(|b| detect_board_texture(b).is_paired)
                        .unwrap_or(false)))
        {
            /* Low/Mid flush (e.g. 5d 3d in Match 1 on 4-flush river or paired 3-flush turn):
             * Vulnerable to higher flushes and full houses -> suppress overbets, shoves, and raises facing a bet! */
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a == RAISE_HALF_POT || a == ALL_IN || (a >= RAISE_MIN && to_call > 0) {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] += shifted * 0.70;
            }
        }

        if !is_preflop
            && is_three_flush
            && to_call > 0
            && pot_odds >= 0.16
            && bucket < 79
            && bucket != 72
        {
            /* Facing a sizable bet on a 3-flush board without a made flush (and without Set+Flush redraw):
             * suppress re-raising / jamming (e.g. Hand 1 with bare trips A3o on 3-spade turn facing 150% overbet),
             * shifting raise probability into flat calling or folding. */
            let mut shifted_raise = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted_raise += probs[i];
                    probs[i] = 0.0;
                }
            }
            if shifted_raise > 0.0 {
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] += shifted_raise * 0.70;
                }
            }
        }

        /* 5. Anti-Passive / Calling-Station Value-Extraction Boost */
        if is_passive_station {
            self.apply_passive_station_boost(
                probs,
                legal_actions,
                bucket,
                round,
                to_call,
                board,
                bb,
                is_three_flush,
                is_four_board_danger,
            );
        }

        /* 6. Postflop Heavy-Bet / Multi-Street Barrel Defense & Unconditional Pot-Commitment Rescue */
        self.apply_postflop_barrel_and_pot_commitment_guards(
            probs,
            legal_actions,
            bucket,
            round,
            to_call,
            bb,
            pot_odds,
            is_three_flush,
            never_fold_postflop,
        );
    }
}
