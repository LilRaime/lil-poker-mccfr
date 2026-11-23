/*! Priority Tier 2B: Postflop Hard Raise Caps, Draw Odds & Pot Control */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::helpers::{
    action_idx, apply_overbet_allin_cap, apply_river_bluff_cap, first_action_idx, force_action,
    zero_and_sum,
};
use crate::cfr::abstraction::{
    BUCKET_FLUSH_MIN, BUCKET_SECOND_NUT_FLUSH, BUCKET_STRAIGHT_MAX, BUCKET_STRAIGHT_MIN,
    BUCKET_STRONG_DRAW_MIN, BUCKET_WEAK_DRAW_MIN, BUCKET_WEAK_PAIR_MIN,
};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, ROUND_FLOP, ROUND_RIVER,
    ROUND_TURN,
};

impl OpponentTracker {
    pub(super) fn apply_tier2_postflop_caps_and_pot_control(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.is_preflop {
            return;
        }
        apply_overbet_allin_cap(self, probs, ctx, reasons);
        apply_river_bluff_cap(self, probs, ctx, false, reasons);
        self.apply_allin_bluff_and_air_gutshot_guards(probs, ctx, reasons);
        self.apply_turn_draw_odds_guard(probs, ctx, reasons);
        self.apply_free_pot_control_caps(probs, ctx);
        self.apply_facing_bet_pot_control_caps(probs, ctx);
        self.apply_straight_flush_and_river_overbet_caps(probs, ctx, reasons);
        self.apply_three_flush_no_raise_cap(probs, ctx, reasons);
    }

    fn apply_allin_bluff_and_air_gutshot_guards(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.bucket < 25
            && !(ctx.round == ROUND_FLOP && (22..BUCKET_WEAK_PAIR_MIN).contains(&ctx.bucket))
            && self.is_guard_enabled("POST_ALLIN_BLUFF_CAP")
            && let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN)
        {
            let removed = probs[ai_i];
            if removed > 0.0 {
                probs[ai_i] = 0.0;
                if let Some(t_i) = first_action_idx(
                    ctx.legal_actions,
                    &[RAISE_HALF_POT, RAISE_THIRD_POT, CALL_CHECK],
                ) {
                    probs[t_i] += removed;
                }
            }
        }

        if (ctx.bucket < BUCKET_STRONG_DRAW_MIN
            || ctx.bucket == BUCKET_WEAK_PAIR_MIN
            || ctx.bucket == 29)
            && self.is_guard_enabled("POST_AIR_GUTSHOT_GUARD")
        {
            if let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN) {
                probs[ai_i] = 0.0;
            }
            if ctx.to_call > 0
                && (ctx.bucket < BUCKET_STRONG_DRAW_MIN || ctx.bucket == BUCKET_WEAK_PAIR_MIN)
            {
                let shifted_raise = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
                let is_flop_small_overcard_float = ctx.round == ROUND_FLOP
                    && ctx.active_opponents == 1
                    && !ctx.is_three_flush
                    && ((ctx.to_call <= ctx.bb * 4
                        && ctx.pot_odds <= 0.31
                        && (7..BUCKET_STRONG_DRAW_MIN).contains(&ctx.bucket))
                        || (ctx.to_call <= ctx.bb * 9
                            && ctx.pot_odds <= 0.28
                            && (12..BUCKET_STRONG_DRAW_MIN).contains(&ctx.bucket)
                            && (ctx.my_chips <= 0 || ctx.to_call * 5 < ctx.my_chips)));
                let overpriced = !is_flop_small_overcard_float
                    && ((ctx.round < ROUND_RIVER
                        && ctx.to_call >= ctx.bb * 2
                        && ctx.pot_odds >= 0.25)
                        || (ctx.round == ROUND_TURN
                            && ctx.to_call >= ctx.bb
                            && ctx.pot_odds >= 0.16
                            && ctx.bucket < BUCKET_STRONG_DRAW_MIN)
                        || (ctx.round < ROUND_RIVER
                            && ctx.to_call >= ctx.bb
                            && ctx.pot_odds > 0.12
                            && (ctx.bucket < BUCKET_WEAK_DRAW_MIN
                                || ctx.bucket == BUCKET_WEAK_PAIR_MIN)));
                if overpriced && force_action(probs, ctx.legal_actions, FOLD) {
                    reasons.push("POST_AIR_GUTSHOT_GUARD");
                } else if !overpriced && shifted_raise > 0.0 {
                    let target = if ctx.pot_odds <= 0.16 || is_flop_small_overcard_float {
                        CALL_CHECK
                    } else {
                        FOLD
                    };
                    if let Some(t_i) = action_idx(ctx.legal_actions, target) {
                        probs[t_i] += shifted_raise;
                    }
                }
            }
        }
    }

    fn apply_turn_draw_odds_guard(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.round == ROUND_FLOP
            && !ctx.is_overbet_bluffer
            && ctx.to_call >= ctx.bb * 4
            && (BUCKET_STRONG_DRAW_MIN..=21).contains(&ctx.bucket)
            && self.is_guard_enabled("FLOP_DRAW_ODDS_FOLD")
        {
            let max_flop_draw_odds = if ctx.bucket <= 18 { 0.34 } else { 0.38 };
            if ctx.pot_odds > max_flop_draw_odds && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.push("FLOP_DRAW_ODDS_FOLD");
                return;
            }
        }
        if ctx.round != ROUND_TURN
            || ctx.to_call < ctx.bb * 2
            || !(BUCKET_STRONG_DRAW_MIN..BUCKET_WEAK_PAIR_MIN).contains(&ctx.bucket)
            || !self.is_guard_enabled("TURN_DRAW_ODDS_FOLD")
        {
            return;
        }
        let max_turn_draw_odds = if ctx.bucket >= 26 {
            0.40
        } else if ctx.bucket >= 24 {
            0.31
        } else if ctx.bucket >= 19 {
            0.22
        } else {
            0.19
        };
        if ctx.pot_odds > max_turn_draw_odds && force_action(probs, ctx.legal_actions, FOLD) {
            reasons.push("TURN_DRAW_ODDS_FOLD");
        }
    }

    fn apply_free_pot_control_caps(&self, probs: &mut [f64], ctx: &GuardContext<'_>) {
        if ctx.round == ROUND_FLOP
            && ctx.to_call == 0
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            let clean_hu_3bet_cbet = ctx.active_opponents == 1
                && !ctx.is_sticky_caller
                && !ctx.is_passive_station
                && !ctx.is_three_flush
                && (ctx.my_chips == 0 || ctx.my_chips > ctx.pot * 2);
            let air_cap = if clean_hu_3bet_cbet {
                7
            } else {
                BUCKET_STRONG_DRAW_MIN
            };
            let should_cap_flop = (ctx.bucket < air_cap && ctx.pot >= ctx.bb * 8)
                || ((BUCKET_WEAK_PAIR_MIN..=37).contains(&ctx.bucket) && ctx.pot >= ctx.bb * 10)
                || (ctx.is_sticky_caller && ctx.bucket < 7);
            if should_cap_flop {
                let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
                if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                    let floor = if ctx.is_sticky_caller && ctx.pot < ctx.bb * 8 {
                        0.90
                    } else {
                        0.95
                    };
                    probs[c_i] = (probs[c_i] + shifted).max(floor);
                }
            }
            if ctx.pot >= ctx.bb * 8 && (BUCKET_STRONG_DRAW_MIN..=21).contains(&ctx.bucket) {
                let oversized = zero_and_sum(probs, ctx.legal_actions, |a| {
                    a == RAISE_HALF_POT || a == ALL_IN
                });
                if oversized > 0.0 {
                    match (
                        first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, RAISE_MIN]),
                        action_idx(ctx.legal_actions, CALL_CHECK),
                    ) {
                        (Some(s_i), Some(c_i)) => {
                            probs[s_i] += oversized * 0.50;
                            probs[c_i] += oversized * 0.50;
                        }
                        (Some(s_i), None) => probs[s_i] += oversized,
                        (None, Some(c_i)) => probs[c_i] += oversized,
                        (None, None) => {}
                    }
                }
            }
        }

        if ctx.round >= ROUND_TURN
            && ctx.to_call == 0
            && ctx.is_four_board_danger
            && ctx.bucket < BUCKET_STRAIGHT_MIN
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        } else if ctx.round >= ROUND_TURN
            && ctx.to_call == 0
            && (ctx.bucket < BUCKET_STRONG_DRAW_MIN
                || (ctx.round == ROUND_RIVER && ctx.bucket < BUCKET_WEAK_PAIR_MIN))
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            if let Some(hp_i) = action_idx(ctx.legal_actions, RAISE_HALF_POT) {
                probs[hp_i] = 0.0;
            }
            let allow_clean_hu_river_stab = ctx.round == ROUND_RIVER
                && ctx.active_opponents == 1
                && !ctx.is_paired_board
                && !ctx.is_three_flush
                && !ctx.is_four_board_danger
                && !ctx.is_passive_station
                && !ctx.is_sticky_caller
                && ctx.bucket <= 6;
            let air_pot_cap = if allow_clean_hu_river_stab {
                ctx.bb * 14
            } else if ctx.bucket < BUCKET_WEAK_DRAW_MIN {
                ctx.bb * 8
            } else {
                ctx.bb * 18
            };
            let should_cap_bluff = (ctx.is_passive_station
                || ctx.is_sticky_caller
                || ctx.active_opponents >= 2
                || ctx.pot >= air_pot_cap
                || (ctx.is_three_flush && ctx.bucket < BUCKET_STRONG_DRAW_MIN))
                && !ctx.has_river_nut_blocker;
            if should_cap_bluff {
                let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
                if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                    probs[c_i] = (probs[c_i] + shifted).max(0.95);
                }
            }
        } else if ctx.round == ROUND_TURN
            && ctx.to_call == 0
            && (BUCKET_STRONG_DRAW_MIN..BUCKET_WEAK_PAIR_MIN).contains(&ctx.bucket)
            && (ctx.is_passive_station || ctx.active_opponents >= 2)
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            if let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN) {
                probs[ai_i] = 0.0;
            }
            if ctx.is_passive_station && ctx.is_three_flush {
                let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
                if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                    probs[c_i] = (probs[c_i] + shifted).max(0.90);
                }
            }
        } else if ctx.round >= ROUND_TURN
            && ctx.to_call == 0
            && ctx.pot >= ctx.bb * 8
            && (BUCKET_WEAK_PAIR_MIN..=38).contains(&ctx.bucket)
            && !(ctx.is_passive_station && ctx.bucket >= 35 && !ctx.is_three_flush)
            && (ctx.round == ROUND_TURN
                || ctx.bucket <= 33
                || ctx.is_paired_board
                || ctx.is_three_flush
                || ctx.is_four_board_danger)
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        }
    }

    fn apply_facing_bet_pot_control_caps(&self, probs: &mut [f64], ctx: &GuardContext<'_>) {
        /* Pair + Strong Draw Defense on Flop & Turn (bucket 33 or 38) */
        if ctx.round < ROUND_RIVER
            && ctx.to_call > 0
            && (ctx.bucket == 33 || ctx.bucket == 38)
            && ctx.pot_odds <= 0.35
            && !ctx.is_three_flush
            && let (Some(f_i), Some(c_i)) = (
                action_idx(ctx.legal_actions, FOLD),
                action_idx(ctx.legal_actions, CALL_CHECK),
            )
        {
            let shift = probs[f_i] * 0.85;
            probs[f_i] -= shift;
            probs[c_i] = (probs[c_i] + shift).max(0.80);
        }

        /* Flop facing a significant bet: call only with weak/medium pairs & bare draws instead of check-raising */
        let flop_pair_cap_max =
            if ctx.is_known_maniac || ctx.is_overbet_bluffer || ctx.is_hyper_aggressor {
                43
            } else {
                46
            };
        if ctx.round == ROUND_FLOP
            && (((BUCKET_WEAK_PAIR_MIN..=flop_pair_cap_max).contains(&ctx.bucket)
                && ctx.to_call >= ctx.bb * 2
                && ctx.pot_odds >= 0.24)
                || ((BUCKET_STRONG_DRAW_MIN..=21).contains(&ctx.bucket)
                    && ctx.to_call > 0
                    && (ctx.to_call >= ctx.bb * 4 || ctx.pot_odds >= 0.30)))
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += shifted;
            }
        }

        /* Turn facing a bet: never probe-raise or check-raise with weak or marginal hands (< 61) */
        if ctx.round == ROUND_TURN
            && ctx.to_call > 0
            && ctx.bucket < 61
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if shifted > 0.0
                && let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK)
            {
                probs[c_i] += shifted;
            }
        }
    }

    fn apply_straight_flush_and_river_overbet_caps(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.round >= ROUND_TURN
            && ctx.bucket == BUCKET_STRAIGHT_MIN
            && self.is_guard_enabled("POST_PAIRED_FLUSH_STRAIGHT_CAP")
        {
            Self::cap_idiot_end_straight(probs, ctx, reasons);
        } else if ctx.round >= ROUND_TURN
            && ctx.bucket == 74
            && (ctx.to_call >= ctx.bb * 3
                || ctx.pot + ctx.to_call >= ctx.bb * 12
                || ctx.is_three_flush)
            && self.is_guard_enabled("POST_PAIRED_FLUSH_STRAIGHT_CAP")
        {
            reasons.push("POST_PAIRED_FLUSH_STRAIGHT_CAP");
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| {
                a == ALL_IN
                    || (a == RAISE_HALF_POT && (ctx.to_call > 0 || ctx.is_three_flush))
                    || (a >= RAISE_MIN && ctx.to_call >= ctx.bb * 3)
            });
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.60);
            }
        } else if ctx.round == ROUND_RIVER
            && (74..=BUCKET_STRAIGHT_MAX).contains(&ctx.bucket)
            && (ctx.is_three_flush || ctx.is_paired_board || ctx.is_four_flush_board)
            && self.is_guard_enabled("POST_PAIRED_FLUSH_STRAIGHT_CAP")
        {
            Self::cap_dangerous_river_straight(probs, ctx, reasons);
        } else if ctx.round == ROUND_RIVER
            && (BUCKET_WEAK_PAIR_MIN..=72).contains(&ctx.bucket)
            && self.is_guard_enabled("POST_RIVER_OVERBET_CAP")
        {
            Self::cap_dangerous_river_overbets(probs, ctx);
        } else if ctx.round >= ROUND_TURN
            && ((BUCKET_FLUSH_MIN..=81).contains(&ctx.bucket)
                || (ctx.is_four_flush_board
                    && ctx.to_call > 0
                    && ctx.bucket == BUCKET_SECOND_NUT_FLUSH
                    && (ctx.to_call >= ctx.bb * 4 || ctx.pot_odds >= 0.25)))
            && (ctx.is_four_flush_board || (ctx.is_three_flush && ctx.is_paired_board))
            && self.is_guard_enabled("POST_PAIRED_FLUSH_STRAIGHT_CAP")
        {
            reasons.push("POST_PAIRED_FLUSH_STRAIGHT_CAP");
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| {
                a == RAISE_HALF_POT || a == ALL_IN || (a >= RAISE_MIN && ctx.to_call > 0)
            });
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += if ctx.is_four_flush_board && ctx.to_call > 0 {
                    shifted
                } else {
                    shifted * 0.70
                };
            }
        }
    }

    fn cap_idiot_end_straight(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        reasons.push("POST_PAIRED_FLUSH_STRAIGHT_CAP");
        let shifted = zero_and_sum(probs, ctx.legal_actions, |a| {
            a == RAISE_HALF_POT || a == ALL_IN || (a >= RAISE_MIN && ctx.to_call > 0)
        });
        if ctx.to_call == 0 && ctx.round == ROUND_TURN {
            if let Some(r_t) = first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, RAISE_MIN]) {
                probs[r_t] += shifted * 0.75;
            }
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += shifted * 0.25;
            }
        } else if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
            probs[c_i] = (probs[c_i] + shifted).max(0.55);
        }
    }

    fn cap_dangerous_river_straight(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        reasons.push("POST_PAIRED_FLUSH_STRAIGHT_CAP");
        let shifted = zero_and_sum(probs, ctx.legal_actions, |a| {
            a == RAISE_HALF_POT || a == ALL_IN || (a >= RAISE_MIN && ctx.to_call > 0)
        });
        if ctx.to_call == 0 && !ctx.is_three_flush && !ctx.is_four_flush_board {
            if let Some(r_t) = action_idx(ctx.legal_actions, RAISE_THIRD_POT) {
                probs[r_t] = (probs[r_t] + shifted * 0.65).max(0.55);
            }
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += shifted * 0.35;
            }
        } else if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
            probs[c_i] = (probs[c_i] + shifted).max(0.65);
        }
    }

    fn cap_dangerous_river_overbets(probs: &mut [f64], ctx: &GuardContext<'_>) {
        let is_dangerous =
            ctx.is_dangerous_board || ctx.is_three_flush || ctx.is_three_straight_board;
        if !is_dangerous {
            return;
        }
        if ctx.to_call > 0 && (ctx.bucket < 61 || ctx.to_call >= ctx.bb * 4) {
            let shifted_raise = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if shifted_raise > 0.0
                && let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK)
            {
                probs[c_i] += shifted_raise;
            }
        } else if ctx.to_call == 0 && ctx.bucket < 67 {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| {
                a == RAISE_HALF_POT || a == ALL_IN
            });
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.60);
            }
        }
    }

    fn apply_three_flush_no_raise_cap(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if !ctx.is_three_flush
            || ctx.to_call <= 0
            || ctx.pot_odds < 0.16
            || ctx.bucket >= BUCKET_FLUSH_MIN
            || ctx.bucket == 72
            || !self.is_guard_enabled("THREE_FLUSH_NO_RAISE")
        {
            return;
        }
        let shifted_raise = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
        if shifted_raise > 0.0 {
            reasons.push("THREE_FLUSH_NO_RAISE");
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += shifted_raise * 0.70;
            }
        }
    }
}
