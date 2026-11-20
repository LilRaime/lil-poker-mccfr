/*! SubgameSolver Turn/River (round >= 3) Essential Safety Invariants */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::helpers::{
    action_idx, apply_overbet_allin_cap, apply_pot_commit_terminal, apply_river_bluff_cap,
    first_action_idx, force_action, shift_action_to, zero_and_sum,
};
use crate::game::config::{ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT};

impl OpponentTracker {
    pub(super) fn apply_subgame_postflop_guards(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        apply_overbet_allin_cap(self, probs, ctx, reasons);
        self.apply_subgame_reraise_cap(probs, ctx, reasons);
        if ctx.bucket < 15 && self.is_guard_enabled("POST_ALLIN_BLUFF_CAP") {
            shift_action_to(probs, ctx.legal_actions, ALL_IN, &[CALL_CHECK]);
        }
        apply_river_bluff_cap(self, probs, ctx, true, reasons);
        self.apply_subgame_nut_value_floor(probs, ctx, reasons);
        self.apply_subgame_four_board_fold(probs, ctx, reasons);
        self.apply_subgame_barrel_fold(probs, ctx, reasons);
        self.apply_subgame_overbet_bluff_catch(probs, ctx, reasons);
        apply_pot_commit_terminal(self, probs, ctx, reasons);
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
        let cap_one_pair = ctx.bucket < 57
            && (ctx.is_paired_board
                || ctx.is_three_flush
                || ctx.is_four_board_danger
                || ctx.to_call * 3 >= ctx.pot * 2
                || ctx.to_call >= ctx.bb * 4);
        let cap_four_board_river = ctx.round == 4 && ctx.is_four_board_danger && ctx.bucket < 73;
        if !cap_one_pair && !cap_four_board_river {
            return;
        }
        let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
        if shifted > 0.0 {
            reasons.push("POST_SUBGAME_RERAISE_CAP");
            let primary = if ctx.bucket < 15 || ctx.bucket == 28 {
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
        if ctx.round != 4
            || ctx.to_call != 0
            || ctx.bucket < 75
            || (ctx.is_paired_board && ctx.bucket <= 78)
            || (ctx.is_four_flush_board && ctx.bucket <= 78)
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
            && ctx.bucket <= 78
            && ((ctx.round == 4 && ctx.to_call >= ctx.bb && ctx.pot_odds >= 0.20)
                || (ctx.round == 3 && ctx.to_call >= ctx.bb * 3 && ctx.pot_odds >= 0.28));
        let fold_four_straight = ctx.round == 4
            && ctx.is_four_straight_board
            && ctx.bucket < 73
            && ctx.to_call >= ctx.bb * 4
            && ctx.pot_odds >= 0.30
            && (ctx.bucket <= 40 || !ctx.is_overbet_bluffer);
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
        let fold_river_barrel = ctx.round == 4
            && ctx.to_call >= ctx.bb * 4
            && ctx.pot_odds >= 0.24
            && ((ctx.bucket <= 30 && !ctx.is_overbet_bluffer)
                || (ctx.is_tight_passive_postflop && ctx.bucket <= 40)
                || (ctx.is_tight_passive_postflop
                    && ctx.is_paired_board
                    && ctx.is_dangerous_board
                    && ctx.bucket <= 43
                    && ctx.to_call >= ctx.bb * 10));
        let fold_turn_barrel = ctx.round == 3
            && ctx.to_call >= ctx.bb * 5
            && ctx.pot_odds >= 0.33
            && ctx.bucket <= 30
            && !ctx.is_overbet_bluffer;
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
        if !ctx.is_overbet_bluffer
            || ctx.to_call <= 0
            || ctx.is_four_flush_board
            || ctx.is_four_board_danger
            || ctx.bucket < 29
            || ctx.to_call * 10 <= (ctx.pot - ctx.to_call).max(1) * 12
            || !self.is_guard_enabled("OVERBET_BLUFF_CATCH")
        {
            return;
        }
        if let (Some(f_i), Some(c_i)) = (
            action_idx(ctx.legal_actions, FOLD),
            action_idx(ctx.legal_actions, CALL_CHECK),
        ) {
            if probs[f_i] > 0.35 {
                reasons.push("OVERBET_BLUFF_CATCH");
                let s = probs[f_i] * 0.65;
                probs[f_i] -= s;
                probs[c_i] += s;
            }
        }
    }
}
