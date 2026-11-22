/*! `SubgameSolver` Turn/River (`round >= 3`) Essential Safety Invariants */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::helpers::{
    action_idx, apply_overbet_allin_cap, apply_pot_commit_terminal, apply_river_bluff_cap,
    first_action_idx, force_action, shift_action_to, zero_and_sum,
};
use crate::cfr::abstraction::{
    BUCKET_FLUSH_MIN, BUCKET_OVERPAIR_MIN, BUCKET_STRAIGHT_MAX, BUCKET_STRAIGHT_MIN,
    BUCKET_STRONG_DRAW_MIN, BUCKET_TOP_PAIR_MIN, BUCKET_TRIPS_MIN, BUCKET_TWO_PAIR_MIN,
    BUCKET_WEAK_DRAW_MIN, BUCKET_WEAK_PAIR_MIN,
};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, ROUND_RIVER, ROUND_TURN,
};

impl OpponentTracker {
    pub(super) fn apply_subgame_postflop_guards(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        apply_overbet_allin_cap(self, probs, ctx, reasons);
        self.apply_subgame_reraise_cap(probs, ctx, reasons);
        if ctx.bucket < BUCKET_STRONG_DRAW_MIN && self.is_guard_enabled("POST_ALLIN_BLUFF_CAP") {
            shift_action_to(probs, ctx.legal_actions, ALL_IN, &[CALL_CHECK]);
        }
        apply_river_bluff_cap(self, probs, ctx, true, reasons);
        self.apply_subgame_free_pot_control_cap(probs, ctx);
        self.apply_subgame_nut_value_floor(probs, ctx, reasons);
        self.apply_subgame_air_and_draw_odds_fold(probs, ctx, reasons);
        self.apply_subgame_four_board_fold(probs, ctx, reasons);
        self.apply_subgame_shove_defense(probs, ctx, reasons);
        self.apply_subgame_barrel_fold(probs, ctx, reasons);
        self.apply_subgame_overbet_bluff_catch(probs, ctx, reasons);
        apply_pot_commit_terminal(self, probs, ctx, reasons);
    }

    fn apply_subgame_free_pot_control_cap(&self, probs: &mut [f64], ctx: &GuardContext<'_>) {
        if ctx.to_call != 0 || !self.is_guard_enabled("POST_POT_CONTROL_CAP") {
            return;
        }
        if ctx.is_four_board_danger && ctx.bucket < BUCKET_STRAIGHT_MIN {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        } else if (ctx.bucket < BUCKET_STRONG_DRAW_MIN
            || (ctx.round == ROUND_RIVER && ctx.bucket < BUCKET_WEAK_PAIR_MIN))
            && !ctx.has_river_nut_blocker
        {
            if let Some(hp_i) = action_idx(ctx.legal_actions, RAISE_HALF_POT) {
                probs[hp_i] = 0.0;
            }
            let air_pot_cap = if ctx.bucket < BUCKET_WEAK_DRAW_MIN {
                ctx.bb * 8
            } else {
                ctx.bb * 18
            };
            if ctx.is_passive_station
                || ctx.is_sticky_caller
                || ctx.active_opponents >= 2
                || ctx.pot >= air_pot_cap
                || (ctx.is_three_flush && ctx.bucket < BUCKET_STRONG_DRAW_MIN)
            {
                let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
                if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                    probs[c_i] = (probs[c_i] + shifted).max(0.95);
                }
            }
        } else if ctx.pot >= ctx.bb * 5
            && (BUCKET_WEAK_PAIR_MIN..=38).contains(&ctx.bucket)
            && !(ctx.is_passive_station && ctx.bucket >= 35 && !ctx.is_three_flush)
        {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        }
    }

    fn apply_subgame_air_and_draw_odds_fold(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.to_call <= 0 || ctx.never_fold_postflop {
            return;
        }
        if (ctx.bucket < BUCKET_STRONG_DRAW_MIN || ctx.bucket == BUCKET_WEAK_PAIR_MIN)
            && self.is_guard_enabled("POST_AIR_GUTSHOT_GUARD")
        {
            let overpriced_air = (ctx.round == ROUND_TURN
                && ctx.to_call >= ctx.bb
                && ((ctx.pot_odds >= 0.16 && ctx.bucket < BUCKET_STRONG_DRAW_MIN)
                    || (ctx.pot_odds > 0.12
                        && (ctx.bucket < BUCKET_WEAK_DRAW_MIN
                            || ctx.bucket == BUCKET_WEAK_PAIR_MIN))))
                || (ctx.round == ROUND_RIVER
                    && ctx.to_call >= ctx.bb * 2
                    && ctx.pot_odds >= 0.18
                    && (ctx.bucket < BUCKET_WEAK_DRAW_MIN || ctx.bucket == BUCKET_WEAK_PAIR_MIN));
            if overpriced_air && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.push("POST_AIR_GUTSHOT_GUARD");
                return;
            }
        }
        if ctx.round == ROUND_TURN
            && ctx.to_call >= ctx.bb * 2
            && (BUCKET_STRONG_DRAW_MIN..BUCKET_WEAK_PAIR_MIN).contains(&ctx.bucket)
            && self.is_guard_enabled("TURN_DRAW_ODDS_FOLD")
        {
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
    }

    fn apply_subgame_shove_defense(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.to_call <= 0
            || ctx.never_fold_postflop
            || !self.is_guard_enabled("SHOVE_DEFENSE_FOLD")
        {
            return;
        }
        let is_subgame_shove =
            (ctx.is_allin_call && ctx.to_call >= ctx.bb * 5 && ctx.pot_odds >= 0.22)
                || (ctx.round == ROUND_RIVER && ctx.to_call >= ctx.bb * 12 && ctx.pot_odds >= 0.30)
                || (ctx.round == ROUND_TURN && ctx.to_call >= ctx.bb * 12 && ctx.pot_odds >= 0.30);
        if !is_subgame_shove {
            return;
        }
        let pre_pot = (ctx.pot - ctx.to_call).max(1);
        let is_extreme_overbet_jam =
            ctx.to_call >= ctx.bb * 20 && (ctx.to_call >= pre_pot * 3 || ctx.pot_odds >= 0.44);
        let is_river_one_pair_jam = ctx.round == ROUND_RIVER
            && !ctx.is_overbet_bluffer
            && ((!ctx.is_known_maniac && ctx.bucket < 44)
                || (ctx.bucket <= 33 && ctx.to_call >= ctx.bb * 10 && ctx.pot_odds >= 0.25)
                || (ctx.is_paired_board
                    && ctx.bucket < BUCKET_OVERPAIR_MIN
                    && ctx.to_call >= ctx.bb * 15
                    && ctx.pot_odds >= 0.23));
        let is_turn_structural_weak_jam = ctx.round == ROUND_TURN
            && ((ctx.is_three_flush && ctx.bucket < BUCKET_TOP_PAIR_MIN)
                || (ctx.bucket <= 33 && ctx.to_call >= ctx.bb * 10 && ctx.pot_odds >= 0.25));
        let is_turn_one_pair_jam = is_turn_structural_weak_jam
            || (ctx.round == ROUND_TURN
                && !ctx.is_overbet_bluffer
                && ((is_extreme_overbet_jam && ctx.bucket < 58)
                    || (ctx.is_paired_board && ctx.bucket < 52)
                    || (ctx.is_three_flush && ctx.bucket <= BUCKET_OVERPAIR_MIN)
                    || (!ctx.is_known_maniac && ctx.bucket < BUCKET_TOP_PAIR_MIN)));
        let mut is_trash = is_river_one_pair_jam || is_turn_one_pair_jam;
        if !is_trash && !ctx.is_overbet_bluffer && self.is_guard_enabled("RANGE_EQUITY_SHOVE_FOLD")
        {
            is_trash = self.is_range_equity_shove_trash(ctx, reasons);
        }
        if is_trash && force_action(probs, ctx.legal_actions, FOLD) {
            reasons.push("SHOVE_DEFENSE_FOLD");
        }
    }

    fn apply_subgame_reraise_cap(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.to_call <= 0
            || ctx.is_allin_call
            || !self.is_guard_enabled("POST_SUBGAME_RERAISE_CAP")
        {
            return;
        }
        let cap_one_pair = ctx.bucket < BUCKET_TWO_PAIR_MIN
            && (ctx.round == ROUND_TURN
                || ctx.is_paired_board
                || ctx.is_three_flush
                || ctx.is_four_board_danger
                || ctx.to_call * 3 >= ctx.pot * 2
                || ctx.to_call >= ctx.bb * 4);
        let cap_four_board_river = ctx.round == ROUND_RIVER
            && ctx.is_four_board_danger
            && ctx.bucket < BUCKET_STRAIGHT_MIN;
        if !cap_one_pair && !cap_four_board_river {
            return;
        }
        let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
        if shifted > 0.0 {
            reasons.push("POST_SUBGAME_RERAISE_CAP");
            let primary =
                if ctx.bucket < BUCKET_STRONG_DRAW_MIN || ctx.bucket == BUCKET_WEAK_PAIR_MIN {
                    FOLD
                } else {
                    CALL_CHECK
                };
            if let Some(t_i) = first_action_idx(ctx.legal_actions, &[primary, CALL_CHECK, FOLD]) {
                probs[t_i] += shifted;
            }
        }
    }

    fn apply_subgame_nut_value_floor(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        let is_river_nut = ctx.round == ROUND_RIVER
            && ctx.bucket >= 75
            && !(ctx.is_paired_board && ctx.bucket <= BUCKET_STRAIGHT_MAX)
            && !(ctx.is_four_flush_board && ctx.bucket <= BUCKET_STRAIGHT_MAX);
        let is_low_spr_two_pair_plus = ctx.round >= ROUND_TURN
            && ctx.bucket >= 60
            && !(ctx.is_paired_board && ctx.bucket < BUCKET_TRIPS_MIN)
            && !ctx.is_four_board_danger
            && ctx.my_chips > 0
            && ctx.my_chips * 5 <= ctx.pot * 6;
        if ctx.to_call != 0
            || (!is_river_nut && !is_low_spr_two_pair_plus)
            || ctx.is_three_flush
            || !self.is_guard_enabled("POST_NUT_VALUE_FLOOR")
        {
            return;
        }
        let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) else {
            return;
        };
        if probs[c_i] <= 0.35 {
            return;
        }
        let target_idx = if ctx.is_four_straight_board {
            first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, RAISE_MIN])
        } else {
            first_action_idx(ctx.legal_actions, &[RAISE_HALF_POT, RAISE_THIRD_POT])
        };
        if let Some(t_i) = target_idx {
            let shift = (probs[c_i] - 0.15).max(0.0) * 0.75;
            if shift > 0.0 {
                reasons.push("POST_NUT_VALUE_FLOOR");
                probs[c_i] -= shift;
                probs[t_i] += shift;
            }
        }
    }

    fn apply_subgame_four_board_fold(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.to_call <= 0
            || ctx.never_fold_postflop
            || !self.is_guard_enabled("SHOVE_DEFENSE_FOLD")
        {
            return;
        }
        let fold_four_flush = ctx.is_four_flush_board
            && ((ctx.bucket <= BUCKET_STRAIGHT_MAX
                && ((ctx.round == ROUND_RIVER && ctx.to_call >= ctx.bb && ctx.pot_odds >= 0.20)
                    || (ctx.round == ROUND_TURN
                        && ctx.to_call >= ctx.bb * 3
                        && ctx.pot_odds >= 0.28)))
                || (ctx.round == ROUND_RIVER
                    && ctx.bucket == BUCKET_FLUSH_MIN
                    && !ctx.is_overbet_bluffer
                    && ctx.to_call >= ctx.bb * 4
                    && ctx.pot_odds >= 0.25));
        let fold_four_straight = ctx.is_four_straight_board
            && ((ctx.round == ROUND_RIVER
                && ctx.bucket < BUCKET_STRAIGHT_MIN
                && ctx.to_call >= ctx.bb * 4
                && ctx.pot_odds >= 0.30
                && (ctx.bucket <= BUCKET_TOP_PAIR_MIN || !ctx.is_overbet_bluffer))
                || (ctx.round == ROUND_TURN
                    && ctx.bucket < BUCKET_TWO_PAIR_MIN
                    && ctx.to_call >= ctx.bb * 4
                    && ctx.pot_odds >= 0.26));
        if (fold_four_flush || fold_four_straight) && force_action(probs, ctx.legal_actions, FOLD) {
            reasons.push("SHOVE_DEFENSE_FOLD");
        }
    }

    fn apply_subgame_barrel_fold(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.to_call <= 0 || ctx.never_fold_postflop || !self.is_guard_enabled("POST_BARREL_FOLD")
        {
            return;
        }
        let fold_river_barrel = ctx.round == ROUND_RIVER
            && ctx.to_call >= ctx.bb * 4
            && ctx.pot_odds >= 0.24
            && ((ctx.bucket <= 30 && !ctx.is_overbet_bluffer)
                || (ctx.is_tight_passive_postflop && ctx.bucket <= BUCKET_TOP_PAIR_MIN)
                || (ctx.is_tight_passive_postflop
                    && ctx.is_paired_board
                    && ctx.is_dangerous_board
                    && ctx.bucket <= 43
                    && ctx.to_call >= ctx.bb * 10));
        let fold_turn_barrel = ctx.round == ROUND_TURN
            && ctx.to_call >= ctx.bb * 5
            && ctx.pot_odds >= 0.28
            && ((ctx.bucket <= 30 && !ctx.is_overbet_bluffer)
                || (ctx.bucket <= 33 && ctx.to_call >= ctx.bb * 10 && ctx.pot_odds >= 0.30)
                || (ctx.is_three_flush
                    && ctx.bucket < BUCKET_TOP_PAIR_MIN
                    && ctx.to_call >= ctx.bb * 6
                    && ctx.pot_odds >= 0.30)
                || (ctx.bucket <= 35
                    && !ctx.is_overbet_bluffer
                    && ctx.to_call >= ctx.bb * 6
                    && ctx.pot_odds >= 0.36));
        if (fold_river_barrel || fold_turn_barrel) && force_action(probs, ctx.legal_actions, FOLD) {
            reasons.push("POST_BARREL_FOLD");
        }
    }

    fn apply_subgame_overbet_bluff_catch(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        let is_overbet_sizing = ctx.to_call * 10 > (ctx.pot - ctx.to_call).max(1) * 12;
        let is_priced_in_river_catch = ctx.round == ROUND_RIVER
            && ctx.pot_odds <= 0.26
            && ctx.to_call >= ctx.bb * 4
            && ctx.bucket >= 29;
        let min_bluff_catch_bucket = if is_priced_in_river_catch {
            29
        } else if ctx.round == ROUND_TURN && ctx.to_call >= ctx.bb * 10 {
            34
        } else {
            29
        };
        if !ctx.is_overbet_bluffer
            || ctx.to_call <= 0
            || ctx.is_four_flush_board
            || ctx.is_four_board_danger
            || (ctx.round == ROUND_TURN && ctx.is_three_flush && ctx.bucket < BUCKET_TOP_PAIR_MIN)
            || ctx.bucket < min_bluff_catch_bucket
            || (!is_overbet_sizing && !is_priced_in_river_catch)
            || !self.is_guard_enabled("OVERBET_BLUFF_CATCH")
        {
            return;
        }
        if let (Some(f_i), Some(c_i)) = (
            action_idx(ctx.legal_actions, FOLD),
            action_idx(ctx.legal_actions, CALL_CHECK),
        ) && probs[f_i] > 0.35
        {
            reasons.push("OVERBET_BLUFF_CATCH");
            let s = probs[f_i] * 0.65;
            probs[f_i] -= s;
            probs[c_i] += s;
        }
    }
}
