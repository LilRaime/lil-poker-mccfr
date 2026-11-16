/* Priority Tier 2A: Preflop Discipline & All-In / Heavy-Bet Shove Defense */
use super::super::tracker::OpponentTracker;
use crate::cfr::abstraction::{
    calculate_range_equity, detect_board_texture, is_dangerous_board, is_four_flush_board,
    is_four_straight_board, nash_call_shove_max_bb, nash_open_push_max_bb, preflop_strength_tier,
};
use crate::game::config::*;
use crate::game::holdem::Card;

impl OpponentTracker {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_tier2_preflop_and_shove_defense(
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
        my_chips: i32,
        eff_allin_threshold: i32,
        pot_odds: f64,
        is_three_flush: bool,
        reasons: &mut Vec<&'static str>,
    ) {
        let is_preflop = round == 1;
        let is_allin_call = my_chips > 0 && to_call >= my_chips;
        let eff_stack_bb = if my_chips > 0 {
            my_chips / bb.max(1)
        } else {
            100
        };
        let is_short_stack = my_chips > 0 && eff_stack_bb <= 25;
        let is_known_maniac = self.total_actions() >= 8
            && (self.raise_ratio() > 0.35 || self.pfr_ratio() > 0.40 || self.vpip_ratio() > 0.65);
        let is_extreme_shove_bot =
            self.total_hands >= 8 && self.raise_ratio() > 0.45 && self.vpip_ratio() > 0.65;
        let can_raise = !is_allin_call
            && legal_actions
                .iter()
                .any(|&a| (RAISE_MIN..=ALL_IN).contains(&a));

        /* Preflop Trash, Priced-In BB Defense & Limp-Pot OOP ISO-Raise Guard */
        if is_preflop && self.is_guard_enabled("PF_DISCIPLINE_GUARD") {
            let pf_tier = preflop_strength_tier(bucket);
            let is_sb_open =
                active_opponents == 1 && to_call > 0 && to_call <= bb / 2 && pot <= bb * 2;
            let max_nash_bb = nash_call_shove_max_bb(
                bucket,
                active_opponents,
                is_known_maniac || is_extreme_shove_bot,
            );
            let short_nash_call_ok = (is_allin_call || (is_short_stack && to_call >= bb * 6))
                && to_call <= max_nash_bb * bb
                && (pf_tier <= 4 || (is_known_maniac && pf_tier <= 5));
            if to_call == 0 && pf_tier >= 6 {
                reasons.push("PF_DISCIPLINE_GUARD");
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
            } else if short_nash_call_ok {
                /* Within Nash push/call depth on short stack: do not fold away profitable calls */
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
            } else if is_sb_open
                && eff_stack_bb <= 20
                && (is_known_maniac || is_extreme_shove_bot)
                && pf_tier >= 6
                && nash_open_push_max_bb(bucket, active_opponents) < eff_stack_bb
            {
                /* Short-stack SB vs hyper-aggressor: fold weak non-Nash hands rather than
                 * limp-folding or 2.5x min-raise-folding to frequent BB iso-shoves. */
                reasons.push("PF_DISCIPLINE_GUARD");
                if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a != FOLD {
                            probs[i] = 0.0;
                        }
                    }
                    probs[f_i] = 1.0;
                }
            } else if is_sb_open && pf_tier >= 7 {
                reasons.push("PF_DISCIPLINE_GUARD");
                let is_tight_passive_bb =
                    self.total_hands >= 8 && self.vpip_ratio() < 0.28 && self.pfr_ratio() < 0.18;
                if pf_tier == 8 || is_known_maniac || is_extreme_shove_bot {
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        for p in probs.iter_mut() {
                            *p = 0.0;
                        }
                        probs[f_i] = 1.0;
                    }
                } else if !self.is_passive_station() && !is_tight_passive_bb {
                    /* Avoid SB limp-folding Tier 7 trash at 21-100 BB: fold or steal instead of limp-folding */
                    if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                        let limp_w = probs[c_i];
                        probs[c_i] = 0.0;
                        if let Some(r_i) = legal_actions
                            .iter()
                            .position(|&a| a == RAISE_MIN)
                            .or_else(|| legal_actions.iter().position(|&a| a == RAISE_THIRD_POT))
                        {
                            probs[r_i] += limp_w * 0.20;
                        }
                        if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                            probs[f_i] += limp_w * 0.80;
                        }
                    }
                }
            } else if to_call > 0 && pf_tier >= 7 {
                reasons.push("PF_DISCIPLINE_GUARD");
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if pot_odds <= 0.22 || (pot_odds <= 0.26 && pf_tier <= 7) {
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
                reasons.push("PF_DISCIPLINE_GUARD");
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
            } else if to_call >= bb * 2 && pot >= bb * 5 && bucket > 5 {
                reasons.push("PF_DISCIPLINE_GUARD");
                let pf_tier = preflop_strength_tier(bucket);
                let nash_shove_call_ok = is_allin_call
                    && to_call
                        <= nash_call_shove_max_bb(bucket, active_opponents, is_known_maniac) * bb;
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if nash_shove_call_ok || (pot_odds <= 0.36 && to_call <= bb * 6) {
                    if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                        probs[c_i] += shifted;
                    }
                } else if to_call >= bb * 6
                    && pot_odds > 0.36
                    && (pf_tier >= 4 || active_opponents >= 2)
                {
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                            shifted += probs[c_i];
                            probs[c_i] = 0.0;
                        }
                        probs[f_i] = (probs[f_i] + shifted).max(0.95);
                    }
                } else if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                    probs[f_i] += shifted;
                }
            }
            /* Small/mid pocket pairs (66..22, buckets 8..=12) facing an open raise (>= 1 BB to call):
             * forbid bloated 33%/50%/all-in 3-bets and cap min-3bet frequency; prefer flat-calling to set-mine. */
            if to_call >= bb && (8..=12).contains(&bucket) {
                reasons.push("PF_DISCIPLINE_GUARD");
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_THIRD_POT {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    } else if a == RAISE_MIN && probs[i] > 0.05 {
                        shifted += probs[i] - 0.05;
                        probs[i] = 0.05;
                    }
                }
                if shifted > 0.0 {
                    if pot_odds <= 0.36 && to_call <= bb * 5 {
                        if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                            probs[c_i] += shifted;
                        }
                    } else if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        probs[f_i] += shifted;
                    }
                }
            }
            if eff_stack_bb > 20
                && (to_call <= bb || (to_call <= bb * 4 && (bucket >= 3 || active_opponents >= 2)))
            {
                if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                    let s = probs[ai_i];
                    if s > 0.0 {
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
        }

        let is_substantial_bet = to_call >= bb * 4;
        let is_shove_faced = pot_odds > 0.18
            && ((is_allin_call && to_call >= bb * 2)
                || (is_substantial_bet
                    && (to_call >= eff_allin_threshold
                        || !can_raise
                        || (round == 1 && to_call >= bb * 10)
                        || (round == 4 && to_call >= bb * 5 && pot_odds >= 0.30)
                        || (round == 3 && to_call >= bb * 15 && pot_odds >= 0.33)
                        || (round == 2 && to_call >= bb * 20))));

        if is_shove_faced && self.is_guard_enabled("SHOVE_DEFENSE_FOLD") {
            if is_preflop {
                let pf_tier = preflop_strength_tier(bucket);
                let max_nash_bb = nash_call_shove_max_bb(
                    bucket,
                    active_opponents,
                    is_known_maniac || is_extreme_shove_bot,
                );
                let within_nash_rescue = to_call <= max_nash_bb * bb
                    && to_call < bb * 40
                    && (pf_tier <= 4 || is_known_maniac || is_extreme_shove_bot);
                if to_call >= bb * 40 {
                    /* Facing massive cold shove (>= 40 BB): AA and KK (bucket <= 1) never fold;
                     * against a known maniac / shove-bot, Tiers 1..=2 (AA-99, AK, AQ, AJs, KQs) never fold! */
                    if bucket <= 1 || (is_known_maniac && pf_tier <= 2) {
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
                } else if (pf_tier <= 1
                    || within_nash_rescue
                    || (is_extreme_shove_bot
                        && pf_tier <= 6
                        && pot_odds <= 0.46
                        && to_call < bb * 15))
                    && to_call < eff_allin_threshold * 2
                {
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
                let max_call_bb = nash_call_shove_max_bb(
                    bucket,
                    active_opponents,
                    is_known_maniac || is_extreme_shove_bot,
                );
                if pot_odds <= 0.22 || (pot_odds <= 0.26 && pf_tier <= 7) {
                    false
                } else if to_call >= bb * 40 {
                    if is_known_maniac {
                        pf_tier > 2
                    } else if active_opponents >= 2 {
                        bucket > 1
                    } else {
                        !(bucket <= 2 || bucket == 13)
                    }
                } else if to_call >= bb * 12 {
                    to_call > max_call_bb * bb
                } else if to_call >= eff_allin_threshold {
                    if is_known_maniac {
                        pf_tier > 2
                    } else if active_opponents == 1 {
                        pf_tier > 1 && bucket != 3 && bucket != 14
                    } else {
                        pf_tier > 1
                    }
                } else if is_extreme_shove_bot && pot_odds <= 0.46 {
                    pf_tier > 6
                } else if is_known_maniac {
                    pf_tier > 5
                } else {
                    pf_tier >= 5
                }
            } else if round == 4 {
                let is_four_flush = board.map(is_four_flush_board).unwrap_or(false);
                let is_four_straight = board.map(is_four_straight_board).unwrap_or(false);
                let is_paired_river = board
                    .map(|b| detect_board_texture(b).is_paired)
                    .unwrap_or(false);
                let min_river_shove_bucket = if is_known_maniac
                    && !is_paired_river
                    && !is_three_flush
                    && !is_four_straight
                {
                    47 /* Against a known maniac on a dry river, TPTK / Overpair (47+) can call */
                } else {
                    54
                };
                bucket < min_river_shove_bucket
                    || (to_call >= bb * 20
                        && pot_odds >= 0.35
                        && bucket < 61
                        && !(is_known_maniac
                            && !is_paired_river
                            && !is_three_flush
                            && bucket >= 54))
                    || (to_call >= bb * 25
                        && pot_odds >= 0.35
                        && (is_paired_river || is_three_flush)
                        && bucket <= 66)
                    || (is_four_flush && bucket <= 79)
                    || (is_four_straight && bucket < 73)
            } else {
                let is_wet = board.map(is_dangerous_board).unwrap_or(false);
                let is_paired = board
                    .map(|b| detect_board_texture(b).is_paired)
                    .unwrap_or(false);
                let is_strong_draw = (15..=18).contains(&bucket) || (19..=27).contains(&bucket);
                let draw_can_call_shove = if round == 3 {
                    (bucket >= 24 && pot_odds <= 0.31 && !is_three_flush)
                        || ((bucket == 26 || bucket == 27) && pot_odds <= 0.40)
                        || ((bucket == 22 || bucket == 23) && pot_odds <= 0.22)
                } else {
                    let simple_draw_odds_ok = pot_odds <= 0.30 || to_call < bb * 15;
                    let premium_draw = bucket >= 22;
                    (is_strong_draw
                        && !is_three_flush
                        && (premium_draw || simple_draw_odds_ok)
                        && pot_odds <= 0.35)
                        || ((bucket == 22 || bucket == 23 || bucket >= 25) && pot_odds <= 0.35)
                };

                if draw_can_call_shove {
                    false
                } else {
                    (is_three_flush && bucket <= 55)
                        || (round == 3 && is_paired && bucket < 57)
                        || bucket < 46
                        || (to_call >= bb * 20
                            && pot_odds >= 0.35
                            && bucket <= 47
                            && !(is_known_maniac && !is_wet && !is_paired && bucket >= 47))
                        || (round == 3 && (46..=55).contains(&bucket) && is_wet)
                }
            };

            let mut is_trash = is_trash;
            if !is_preflop && !is_trash && self.is_guard_enabled("RANGE_EQUITY_SHOVE_FOLD") {
                if let (Some(h), Some(b)) = (hole, board) {
                    if b.len() >= 3 {
                        let obs_vpip = if self.total_hands >= 6 {
                            self.vpip_ratio()
                        } else {
                            0.45
                        };
                        let range_eq =
                            calculate_range_equity(h, b, obs_vpip, true, active_opponents);
                        if range_eq + 0.05 < pot_odds && bucket < 68 {
                            reasons.push("RANGE_EQUITY_SHOVE_FOLD");
                            is_trash = true;
                        }
                    }
                }
            }

            if is_trash && legal_actions.contains(&FOLD) {
                reasons.push("SHOVE_DEFENSE_FOLD");
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a == FOLD {
                        probs[i] = 1.0;
                    } else {
                        probs[i] = 0.0;
                    }
                }
            }
        }
    }
}
