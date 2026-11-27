/*! `SubgameSolver` Turn/River (`round >= 3`) Essential Safety Invariants */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::guard_id::{GuardId, GuardSet};
use super::helpers::{
    action_idx, apply_overbet_allin_cap, apply_pot_commit_terminal, apply_river_bluff_cap,
    first_action_idx, force_action, shift_action_to, zero_and_sum,
};
use super::shared_postflop::{
    apply_shared_turn_draw_odds_fold, apply_shared_turn_river_free_pot_control,
    has_three_plus_overcards, is_turn_commit_underpair_fold, is_turn_overpriced_air,
    is_turn_paired_or_flush_underpair_fold, is_unpaired_third_or_worse_pair,
};
use crate::cfr::abstraction::{
    BUCKET_FLUSH_MIN, BUCKET_OVERPAIR_MIN, BUCKET_SECOND_NUT_FLUSH, BUCKET_STRAIGHT_MAX,
    BUCKET_STRAIGHT_MIN, BUCKET_TOP_PAIR_MIN, BUCKET_TRIPS_MIN, BUCKET_TWO_PAIR_MIN,
    BUCKET_WEAK_PAIR_MIN, PostflopBucket,
};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, ROUND_RIVER, ROUND_TURN,
};

impl OpponentTracker {
    pub(super) fn apply_subgame_postflop_guards(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut GuardSet,
    ) {
        apply_overbet_allin_cap(self, probs, ctx, reasons);
        self.apply_subgame_reraise_cap(probs, ctx, reasons);
        if ctx.post_bucket.is_air_or_weak_draw()
            && self.is_guard_enabled(GuardId::PostAllinBluffCap)
        {
            if shift_action_to(probs, ctx.legal_actions, ALL_IN, &[CALL_CHECK]) > 0.0 {
                reasons.insert(GuardId::PostAllinBluffCap);
            }
        } else if self.is_guard_enabled(GuardId::PostAllinBluffCap)
            && (ctx.bucket < 25 || (ctx.is_paired_board && ctx.bucket < BUCKET_WEAK_PAIR_MIN))
            && (ctx.my_chips == 0 || ctx.my_chips * 4 > ctx.pot * 3)
        {
            let targets: &[u8] = if ctx.to_call > 0 {
                &[CALL_CHECK]
            } else {
                &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK]
            };
            if shift_action_to(probs, ctx.legal_actions, ALL_IN, targets) > 0.0 {
                reasons.insert(GuardId::PostAllinBluffCap);
            }
        }
        apply_river_bluff_cap(self, probs, ctx, true, reasons);
        apply_shared_turn_river_free_pot_control(self, probs, ctx);
        self.apply_subgame_nut_value_floor(probs, ctx, reasons);
        self.apply_subgame_air_and_draw_odds_fold(probs, ctx, reasons);
        self.apply_subgame_four_board_fold(probs, ctx, reasons);
        self.apply_subgame_shove_defense(probs, ctx, reasons);
        self.apply_subgame_barrel_fold(probs, ctx, reasons);
        self.apply_subgame_overbet_bluff_catch(probs, ctx, reasons);
        apply_pot_commit_terminal(self, probs, ctx, reasons);
    }

    fn apply_subgame_air_and_draw_odds_fold(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut GuardSet,
    ) {
        if ctx.to_call <= 0 || ctx.never_fold_postflop {
            return;
        }
        let pb = ctx.post_bucket;
        if (pb.is_air_or_weak_draw() || pb.is_crushed_pair())
            && !(ctx.has_turn_one_card_fd
                && (ctx.pot_odds <= 0.28 || (ctx.is_overbet_bluffer && ctx.pot_odds <= 0.34)))
            && self.is_guard_enabled(GuardId::PostAirGutshotGuard)
        {
            let overpriced_air = is_turn_overpriced_air(ctx)
                || (ctx.round == ROUND_RIVER
                    && ctx.to_call >= ctx.bb * 2
                    && ((ctx.pot_odds >= 0.18 && (pb.is_pure_rag_air() || pb.is_crushed_pair()))
                        || (ctx.pot_odds >= 0.28 && pb.is_high_card())));
            if overpriced_air && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.insert(GuardId::PostAirGutshotGuard);
                return;
            }
        }
        apply_shared_turn_draw_odds_fold(self, probs, ctx, reasons);
    }

    fn apply_subgame_shove_defense(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut GuardSet,
    ) {
        if ctx.to_call <= 0
            || ctx.never_fold_postflop
            || !self.is_guard_enabled(GuardId::ShoveDefenseFold)
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
        let is_dry_short_spr_river_bet = ctx.round == ROUND_RIVER
            && ctx.to_call < ctx.bb * 10
            && ctx.pot_odds <= 0.33
            && ctx.to_call <= pre_pot
            && !ctx.is_paired_board
            && !ctx.is_three_flush
            && !ctx.is_dangerous_board
            && !ctx.is_four_board_danger;
        let pb = ctx.post_bucket;
        let river_one_pair_floor = if is_dry_short_spr_river_bet {
            PostflopBucket::CLEAN_MID_PAIR_MIN as usize
        } else {
            PostflopBucket::DRY_WEAK_TOP_PAIR as usize
        };
        let is_extreme_overbet_jam =
            ctx.to_call >= ctx.bb * 20 && (ctx.to_call >= pre_pot * 3 || ctx.pot_odds >= 0.44);
        let is_river_one_pair_jam = ctx.round == ROUND_RIVER
            && !ctx.is_overbet_bluffer
            && ((!ctx.is_known_maniac && ctx.bucket < river_one_pair_floor)
                || (pb.as_u8() <= PostflopBucket::BOTTOM_PAIR_REDRAW
                    && ctx.to_call >= ctx.bb * 10
                    && ctx.pot_odds >= 0.25)
                || (ctx.is_paired_board
                    && ctx.bucket < BUCKET_OVERPAIR_MIN
                    && ctx.to_call >= ctx.bb * 15
                    && ctx.pot_odds >= 0.23));
        let is_turn_structural_weak_jam = ctx.round == ROUND_TURN
            && ((ctx.is_three_flush && ctx.bucket < BUCKET_TOP_PAIR_MIN)
                || ((pb.as_u8() <= PostflopBucket::DRY_WEAK_BOTTOM_PAIR_MAX
                    || (!ctx.is_overbet_bluffer
                        && pb.as_u8() <= PostflopBucket::BOTTOM_PAIR_REDRAW))
                    && ctx.to_call >= ctx.bb * 10
                    && ctx.pot_odds >= 0.25)
                || (ctx.to_call >= ctx.bb * 28
                    && ctx.pot_odds >= 0.36
                    && ctx.bucket < BUCKET_TOP_PAIR_MIN
                    && !pb.has_pair_redraw()));
        let is_turn_one_pair_jam = is_turn_structural_weak_jam
            || (ctx.round == ROUND_TURN
                && !ctx.is_overbet_bluffer
                && ((is_extreme_overbet_jam
                    && pb.as_u8() < PostflopBucket::VULNERABLE_TWO_PAIR_MAX)
                    || (ctx.is_paired_board
                        && pb.as_u8() < PostflopBucket::HIGH_VULNERABLE_OVERPAIR)
                    || (ctx.is_three_flush && ctx.bucket <= BUCKET_OVERPAIR_MIN)
                    || (!ctx.is_known_maniac && ctx.bucket < BUCKET_TOP_PAIR_MIN)));
        let mut is_trash = is_river_one_pair_jam || is_turn_one_pair_jam;
        if !is_trash
            && !ctx.is_overbet_bluffer
            && self.is_guard_enabled(GuardId::RangeEquityShoveFold)
        {
            is_trash = self.is_range_equity_shove_trash(ctx, reasons);
        }
        if is_trash && force_action(probs, ctx.legal_actions, FOLD) {
            reasons.insert(GuardId::ShoveDefenseFold);
        }
    }

    fn apply_subgame_reraise_cap(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut GuardSet,
    ) {
        if ctx.to_call <= 0
            || ctx.is_allin_call
            || !self.is_guard_enabled(GuardId::PostSubgameReraiseCap)
        {
            return;
        }
        let pb = ctx.post_bucket;
        let cap_one_pair = ctx.bucket < BUCKET_TWO_PAIR_MIN
            && (ctx.round == ROUND_TURN
                || ctx.is_paired_board
                || ctx.is_three_flush
                || ctx.is_four_board_danger
                || ctx.to_call * 3 >= ctx.pot * 2
                || ctx.to_call >= ctx.bb * 4);
        let cap_four_board_non_straight =
            ctx.round >= ROUND_TURN && ctx.is_four_board_danger && ctx.bucket < BUCKET_STRAIGHT_MIN;
        let cap_three_flush_non_flush = ctx.is_three_flush
            && ctx.pot_odds >= 0.16
            && ctx.bucket < BUCKET_FLUSH_MIN
            && !pb.has_trips_flush_redraw();
        let cap_dangerous_river_non_nut = ctx.round == ROUND_RIVER
            && (ctx.is_dangerous_board || ctx.is_three_flush || ctx.is_three_straight_board)
            && pb.in_range(
                BUCKET_WEAK_PAIR_MIN as u8,
                PostflopBucket::TRIPS_OR_SET_FLUSH_REDRAW,
            )
            && (pb.as_u8() < PostflopBucket::CLEAN_TWO_PAIR_MIN || ctx.to_call >= ctx.bb * 4);
        let cap_deep_bottom_two_pair = ctx.round == ROUND_TURN
            && pb.is_vulnerable_two_pair()
            && ctx.my_chips > ctx.bb * 55
            && ctx.to_call >= ctx.bb * 5
            && !ctx.is_known_maniac
            && !ctx.is_overbet_bluffer;
        let cap_four_flush_non_nut = ctx.round >= ROUND_TURN
            && ctx.is_four_flush_board
            && (ctx.bucket < BUCKET_SECOND_NUT_FLUSH
                || (ctx.bucket == BUCKET_SECOND_NUT_FLUSH
                    && (ctx.to_call >= ctx.bb * 4 || ctx.pot_odds >= 0.25)));
        let cap_paired_three_flush_non_nut = ctx.round >= ROUND_TURN
            && ctx.is_paired_board
            && ctx.is_three_flush
            && (ctx.bucket <= PostflopBucket::GOOD_TWO_CARD_FLUSH as usize
                || (ctx.bucket == BUCKET_SECOND_NUT_FLUSH
                    && (ctx.to_call >= ctx.bb * 4 || ctx.pot_odds >= 0.24)));
        if !cap_one_pair
            && !cap_four_board_non_straight
            && !cap_three_flush_non_flush
            && !cap_dangerous_river_non_nut
            && !cap_deep_bottom_two_pair
            && !cap_four_flush_non_nut
            && !cap_paired_three_flush_non_nut
        {
            return;
        }
        let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
        if shifted > 0.0 {
            reasons.insert(GuardId::PostSubgameReraiseCap);
            let primary = if pb.is_air_or_weak_draw() || pb.is_crushed_pair() {
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
        reasons: &mut GuardSet,
    ) {
        let pb = ctx.post_bucket;
        let is_river_nut = ctx.round == ROUND_RIVER
            && ((pb.as_u8() >= PostflopBucket::CLEAN_UNPAIRED_STRAIGHT_MIN
                && !(ctx.is_paired_board && ctx.bucket <= BUCKET_STRAIGHT_MAX)
                && !(ctx.is_four_flush_board && ctx.bucket <= BUCKET_STRAIGHT_MAX))
                || (pb.is_trips_or_set() && !ctx.is_four_board_danger && !ctx.is_dangerous_board));
        let is_low_spr_two_pair_plus = ctx.round >= ROUND_TURN
            && pb.as_u8() >= 60
            && !(ctx.is_paired_board && ctx.bucket < BUCKET_TRIPS_MIN)
            && !ctx.is_four_board_danger
            && ctx.my_chips > 0
            && ctx.my_chips * 5 <= ctx.pot * 6;
        let is_three_flush_two_pair_plus = ctx.round >= ROUND_TURN
            && ctx.is_three_flush
            && !ctx.is_four_flush_board
            && !ctx.is_four_board_danger
            && ctx.bucket >= 60
            && ctx.bucket < BUCKET_FLUSH_MIN
            && !(ctx.is_paired_board && ctx.bucket < BUCKET_TRIPS_MIN);
        if ctx.to_call != 0 || !self.is_guard_enabled(GuardId::PostNutValueFloor) {
            return;
        }
        let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) else {
            return;
        };
        if is_three_flush_two_pair_plus {
            if probs[c_i] <= 0.45 {
                return;
            }
            if let Some(t_i) = first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, RAISE_MIN]) {
                let shift = (probs[c_i] - 0.25).max(0.0) * 0.65;
                if shift > 0.0 {
                    reasons.insert(GuardId::PostNutValueFloor);
                    probs[c_i] -= shift;
                    probs[t_i] += shift;
                }
            }
            return;
        }
        if (!is_river_nut && !is_low_spr_two_pair_plus)
            || (ctx.is_three_flush && ctx.bucket < BUCKET_FLUSH_MIN)
        {
            return;
        }
        let (check_thresh, keep_floor, shift_frac) = if is_river_nut {
            (0.20, 0.08, 0.88)
        } else {
            (0.35, 0.15, 0.75)
        };
        if probs[c_i] <= check_thresh {
            return;
        }
        let target_idx = if ctx.is_four_straight_board {
            first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, RAISE_MIN])
        } else {
            first_action_idx(ctx.legal_actions, &[RAISE_HALF_POT, RAISE_THIRD_POT])
        };
        if let Some(t_i) = target_idx {
            let shift = (probs[c_i] - keep_floor).max(0.0) * shift_frac;
            if shift > 0.0 {
                reasons.insert(GuardId::PostNutValueFloor);
                probs[c_i] -= shift;
                probs[t_i] += shift;
            }
        }
    }

    fn apply_subgame_four_board_fold(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut GuardSet,
    ) {
        if ctx.to_call <= 0
            || ctx.never_fold_postflop
            || !self.is_guard_enabled(GuardId::ShoveDefenseFold)
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
            reasons.insert(GuardId::ShoveDefenseFold);
        }
    }

    fn apply_subgame_barrel_fold(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut GuardSet,
    ) {
        if ctx.to_call <= 0
            || ctx.never_fold_postflop
            || !self.is_guard_enabled(GuardId::PostBarrelFold)
        {
            return;
        }
        let pb = ctx.post_bucket;
        let fold_river_barrel = ctx.round == ROUND_RIVER
            && ctx.to_call >= ctx.bb * 4
            && ctx.pot_odds >= 0.24
            && ((pb.as_u8() <= PostflopBucket::DRY_WEAK_BOTTOM_PAIR_MAX
                && !ctx.is_overbet_bluffer)
                || (ctx.is_three_flush
                    && ctx.bucket < BUCKET_TOP_PAIR_MIN
                    && !ctx.is_overbet_bluffer
                    && ctx.to_call >= ctx.bb * 5
                    && ctx.pot_odds >= 0.25)
                || (ctx.is_tight_passive_postflop && ctx.bucket <= BUCKET_TOP_PAIR_MIN)
                || (ctx.is_tight_passive_postflop
                    && ctx.is_paired_board
                    && ctx.is_dangerous_board
                    && pb.as_u8() <= PostflopBucket::VULNERABLE_TOP_PAIR_MAX
                    && ctx.to_call >= ctx.bb * 10));
        let fold_turn_barrel = ctx.round == ROUND_TURN
            && ((ctx.to_call >= ctx.bb * 5
                && ctx.pot_odds >= 0.28
                && ((pb.as_u8() <= PostflopBucket::DRY_WEAK_BOTTOM_PAIR_MAX
                    && !ctx.is_overbet_bluffer)
                    || ((pb.as_u8() <= PostflopBucket::DRY_WEAK_BOTTOM_PAIR_MAX
                        || (!ctx.is_overbet_bluffer
                            && pb.as_u8() <= PostflopBucket::BOTTOM_PAIR_REDRAW))
                        && ctx.to_call >= ctx.bb * 10
                        && ctx.pot_odds >= 0.30)
                    || (ctx.is_three_flush
                        && ctx.bucket < BUCKET_TOP_PAIR_MIN
                        && ctx.to_call >= ctx.bb * 6
                        && ctx.pot_odds >= 0.30)
                    || (pb.as_u8() <= PostflopBucket::CLEAN_MID_PAIR_MIN
                        && !ctx.is_overbet_bluffer
                        && ctx.to_call >= ctx.bb * 6
                        && ctx.pot_odds >= 0.36)
                    || (pb.is_bare_weak_underpair()
                        && has_three_plus_overcards(ctx.hole, ctx.board)
                        && ctx.my_chips > 0
                        && ctx.to_call * 5 >= ctx.my_chips * 2)))
                || (!pb.has_pair_redraw()
                    && is_unpaired_third_or_worse_pair(ctx.hole, ctx.board, ctx.is_paired_board)
                    && !ctx.is_overbet_bluffer
                    && !ctx.is_known_maniac
                    && ctx.to_call >= ctx.bb * 5
                    && ctx.pot_odds >= 0.25)
                || (ctx.is_paired_board
                    && ctx.is_three_flush
                    && pb.as_u8() == 41
                    && !ctx.is_overbet_bluffer
                    && ctx.to_call >= ctx.bb * 4
                    && ctx.pot_odds >= 0.24));
        let fold_turn_low_pfr_barrel = ctx.round == ROUND_TURN
            && ctx.is_low_pfr_aggressor
            && pb.as_u8() <= 36
            && pb.as_u8() != PostflopBucket::BOTTOM_PAIR_REDRAW
            && ctx.to_call >= ctx.bb * 4
            && ctx.pot_odds >= 0.25;
        let rem_after_call = ctx.my_chips.saturating_sub(ctx.to_call);
        let new_pot_after_call = ctx.pot + ctx.to_call;
        let fold_turn_pot_commit_trap = ctx.round == ROUND_TURN
            && !ctx.is_allin_call
            && ctx.my_chips > ctx.to_call
            && pb.as_u8() <= PostflopBucket::CLEAN_MID_PAIR_MIN
            && pb.as_u8() != PostflopBucket::BOTTOM_PAIR_REDRAW
            && !ctx.is_overbet_bluffer
            && !ctx.is_known_maniac
            && ctx.to_call >= ctx.bb * 6
            && ctx.pot_odds >= 0.25
            && rem_after_call * 4 < new_pot_after_call;
        if (fold_river_barrel
            || fold_turn_barrel
            || is_turn_paired_or_flush_underpair_fold(ctx)
            || is_turn_commit_underpair_fold(ctx)
            || fold_turn_low_pfr_barrel
            || fold_turn_pot_commit_trap)
            && force_action(probs, ctx.legal_actions, FOLD)
        {
            reasons.insert(GuardId::PostBarrelFold);
        }
    }

    fn apply_subgame_overbet_bluff_catch(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut GuardSet,
    ) {
        let pb = ctx.post_bucket;
        let is_overbet_sizing = ctx.to_call * 10 > (ctx.pot - ctx.to_call).max(1) * 12;
        let is_priced_in_river_catch = ctx.round == ROUND_RIVER
            && ctx.pot_odds <= 0.26
            && ctx.to_call >= ctx.bb * 4
            && pb.as_u8() >= PostflopBucket::THREE_FLUSH_BOTTOM_PAIR;
        let is_short_allin_river_catch = ctx.round == ROUND_RIVER
            && ctx.is_allin_call
            && ctx.pot_odds <= 0.33
            && pb.as_u8() >= 34
            && !ctx.is_paired_board
            && !ctx.is_three_flush;
        let min_bluff_catch_bucket = if is_priced_in_river_catch {
            PostflopBucket::THREE_FLUSH_BOTTOM_PAIR as usize
        } else if ctx.round == ROUND_TURN
            && !pb.has_pair_redraw()
            && ((ctx.to_call >= ctx.bb * 28 && ctx.pot_odds >= 0.36)
                || has_three_plus_overcards(ctx.hole, ctx.board)
                || (ctx.my_chips > 0 && ctx.to_call * 5 >= ctx.my_chips * 2))
        {
            BUCKET_TOP_PAIR_MIN
        } else if ctx.round == ROUND_TURN && !pb.has_pair_redraw() && ctx.to_call >= ctx.bb * 8 {
            34
        } else {
            PostflopBucket::THREE_FLUSH_BOTTOM_PAIR as usize
        };
        if !ctx.is_overbet_bluffer
            || ctx.to_call <= 0
            || ctx.is_four_flush_board
            || ctx.is_four_board_danger
            || (ctx.round == ROUND_TURN && ctx.is_three_flush && ctx.bucket < BUCKET_TOP_PAIR_MIN)
            || ctx.bucket < min_bluff_catch_bucket
            || (!is_overbet_sizing && !is_priced_in_river_catch && !is_short_allin_river_catch)
            || !self.is_guard_enabled(GuardId::OverbetBluffCatch)
        {
            return;
        }
        if let (Some(f_i), Some(c_i)) = (
            action_idx(ctx.legal_actions, FOLD),
            action_idx(ctx.legal_actions, CALL_CHECK),
        ) && probs[f_i] > 0.35
        {
            reasons.insert(GuardId::OverbetBluffCatch);
            let s = probs[f_i] * 0.75;
            probs[f_i] -= s;
            probs[c_i] += s;
        }
    }
}
