/*! Priority Tier 2B: Postflop Hard Raise Caps, Draw Odds & Pot Control */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::guard_id::{GuardId, GuardSet};
use super::helpers::{
    action_idx, apply_overbet_allin_cap, apply_river_bluff_cap, first_action_idx, force_action,
    zero_and_sum,
};
use super::shared_postflop::{
    apply_shared_turn_draw_odds_fold, apply_shared_turn_river_free_pot_control,
    is_turn_overpriced_air,
};
use crate::cfr::abstraction::{
    BUCKET_FLUSH_MIN, BUCKET_SECOND_NUT_FLUSH, BUCKET_STRAIGHT_MIN, BUCKET_TWO_PAIR_MIN,
    BUCKET_WEAK_PAIR_MIN, PostflopBucket,
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
        reasons: &mut GuardSet,
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
        reasons: &mut GuardSet,
    ) {
        let pb = ctx.post_bucket;
        if ctx.bucket < 25
            && !(ctx.round == ROUND_FLOP && pb.is_nut_or_combo_draw())
            && self.is_guard_enabled(GuardId::PostAllinBluffCap)
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

        if (pb.is_air_or_weak_draw() || pb.is_crushed_or_three_flush_bottom_pair())
            && !(ctx.has_turn_one_card_fd
                && (ctx.pot_odds <= 0.28 || (ctx.is_overbet_bluffer && ctx.pot_odds <= 0.34)))
            && self.is_guard_enabled(GuardId::PostAirGutshotGuard)
        {
            if let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN) {
                probs[ai_i] = 0.0;
            }
            if ctx.to_call > 0 && (pb.is_air_or_weak_draw() || pb.is_crushed_pair()) {
                let shifted_raise = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
                let is_flop_small_overcard_float = ctx.round == ROUND_FLOP
                    && ctx.active_opponents == 1
                    && !ctx.is_three_flush
                    && ((ctx.to_call <= ctx.bb * 4
                        && ctx.pot_odds <= 0.31
                        && pb.as_u8() >= PostflopBucket::STRONG_OVERCARDS_MIN
                        && pb.is_air_or_weak_draw())
                        || (ctx.to_call <= ctx.bb * 9
                            && ctx.pot_odds <= 0.28
                            && ctx.bucket >= 12
                            && pb.is_air_or_weak_draw()
                            && (ctx.my_chips <= 0 || ctx.to_call * 5 < ctx.my_chips))
                        || (ctx.is_overbet_bluffer
                            && pb.as_u8() >= PostflopBucket::STRONG_ACE_KING_MIN
                            && ctx.pot_odds <= 0.40
                            && (ctx.my_chips <= 0 || ctx.to_call * 4 < ctx.my_chips)));
                let overpriced = !is_flop_small_overcard_float
                    && ((ctx.round < ROUND_RIVER
                        && ctx.to_call >= ctx.bb * 2
                        && ctx.pot_odds >= 0.25)
                        || is_turn_overpriced_air(ctx)
                        || (ctx.round == ROUND_FLOP
                            && ctx.to_call >= ctx.bb
                            && ctx.pot_odds > 0.12
                            && (pb.is_high_card() || pb.is_crushed_pair())));
                if overpriced && force_action(probs, ctx.legal_actions, FOLD) {
                    reasons.insert(GuardId::PostAirGutshotGuard);
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
        reasons: &mut GuardSet,
    ) {
        let pb = ctx.post_bucket;
        if ctx.round == ROUND_FLOP
            && !ctx.is_overbet_bluffer
            && ctx.to_call >= ctx.bb * 4
            && pb.is_non_nut_draw()
            && self.is_guard_enabled(GuardId::FlopDrawOddsFold)
        {
            let max_flop_draw_odds = if ctx.bucket <= 18 { 0.34 } else { 0.38 };
            if ctx.pot_odds > max_flop_draw_odds && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.insert(GuardId::FlopDrawOddsFold);
                return;
            }
        }
        apply_shared_turn_draw_odds_fold(self, probs, ctx, reasons);
    }

    fn apply_free_pot_control_caps(&self, probs: &mut [f64], ctx: &GuardContext<'_>) {
        let pb = ctx.post_bucket;
        if ctx.round == ROUND_FLOP
            && ctx.to_call == 0
            && self.is_guard_enabled(GuardId::PostPotControlCap)
        {
            let clean_hu_3bet_cbet = ctx.active_opponents == 1
                && !ctx.is_sticky_caller
                && !ctx.is_passive_station
                && !ctx.is_three_flush
                && (ctx.my_chips == 0 || ctx.my_chips > ctx.pot * 2);
            let air_cap_blocked = if clean_hu_3bet_cbet {
                pb.as_u8() < PostflopBucket::STRONG_OVERCARDS_MIN
            } else {
                pb.is_air_or_weak_draw()
            };
            let should_cap_flop = (air_cap_blocked && ctx.pot >= ctx.bb * 8)
                || (pb.in_range(
                    BUCKET_WEAK_PAIR_MIN as u8,
                    PostflopBucket::DRY_MID_OR_UNDERPAIR_MAX,
                ) && ctx.pot >= ctx.bb * 10)
                || (ctx.is_sticky_caller && pb.is_high_card());
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
            if ctx.pot >= ctx.bb * 8 && pb.is_non_nut_draw() {
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

        apply_shared_turn_river_free_pot_control(self, probs, ctx);
    }

    fn apply_facing_bet_pot_control_caps(&self, probs: &mut [f64], ctx: &GuardContext<'_>) {
        let pb = ctx.post_bucket;
        /* Pair + Strong Draw Defense on Flop & Turn (bucket 33 or 38) */
        if ctx.round < ROUND_RIVER
            && ctx.to_call > 0
            && pb.has_pair_redraw()
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
                45
            } else {
                46
            };
        if ctx.round == ROUND_FLOP
            && (((BUCKET_WEAK_PAIR_MIN..=flop_pair_cap_max).contains(&ctx.bucket)
                && ctx.to_call >= ctx.bb * 2
                && ctx.pot_odds >= 0.24)
                || (pb.is_non_nut_draw()
                    && ctx.to_call > 0
                    && (ctx.to_call >= ctx.bb * 4 || ctx.pot_odds >= 0.30))
                || (ctx.to_call >= ctx.bb * 8
                    && ctx.pot >= ctx.bb * 18
                    && ctx.bucket < BUCKET_TWO_PAIR_MIN))
            && self.is_guard_enabled(GuardId::PostPotControlCap)
        {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += shifted;
            }
        }

        /* Turn/River facing a bet: never probe-raise or check-raise with marginal hands or non-straights on 4-board danger */
        if ((ctx.round == ROUND_TURN && pb.as_u8() < PostflopBucket::CLEAN_TWO_PAIR_MIN)
            || (ctx.round >= ROUND_TURN
                && ctx.is_four_board_danger
                && ctx.bucket < BUCKET_STRAIGHT_MIN))
            && ctx.to_call > 0
            && self.is_guard_enabled(GuardId::PostPotControlCap)
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
        reasons: &mut GuardSet,
    ) {
        let pb = ctx.post_bucket;
        let allow_free_station_value = ctx.to_call == 0
            && ctx.is_passive_station
            && !(ctx.is_three_flush && ctx.is_paired_board)
            && !ctx.is_four_flush_board;
        if ctx.round >= ROUND_TURN
            && pb.is_board_chop_or_idiot_straight()
            && self.is_guard_enabled(GuardId::PostPairedFlushStraightCap)
        {
            Self::cap_idiot_end_straight(probs, ctx, reasons);
        } else if ctx.round >= ROUND_TURN
            && pb.is_paired_board_straight()
            && !allow_free_station_value
            && (ctx.to_call >= ctx.bb * 3
                || ctx.pot + ctx.to_call >= ctx.bb * 12
                || ctx.is_three_flush)
            && self.is_guard_enabled(GuardId::PostPairedFlushStraightCap)
        {
            reasons.insert(GuardId::PostPairedFlushStraightCap);
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| {
                a == ALL_IN
                    || (a == RAISE_HALF_POT && (ctx.to_call > 0 || ctx.is_three_flush))
                    || (a >= RAISE_MIN && ctx.to_call >= ctx.bb * 3)
            });
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.60);
            }
        } else if ctx.round == ROUND_RIVER
            && pb.is_non_idiot_straight()
            && !allow_free_station_value
            && (ctx.is_three_flush || ctx.is_paired_board || ctx.is_four_flush_board)
            && self.is_guard_enabled(GuardId::PostPairedFlushStraightCap)
        {
            Self::cap_dangerous_river_straight(probs, ctx, reasons);
        } else if ctx.round == ROUND_RIVER
            && pb.in_range(
                BUCKET_WEAK_PAIR_MIN as u8,
                PostflopBucket::TRIPS_OR_SET_FLUSH_REDRAW,
            )
            && self.is_guard_enabled(GuardId::PostRiverOverbetCap)
        {
            Self::cap_dangerous_river_overbets(probs, ctx);
        } else if ctx.round >= ROUND_TURN
            && (pb.in_range(BUCKET_FLUSH_MIN as u8, PostflopBucket::GOOD_TWO_CARD_FLUSH)
                || (ctx.is_four_flush_board
                    && ctx.to_call > 0
                    && ctx.bucket == BUCKET_SECOND_NUT_FLUSH
                    && (ctx.to_call >= ctx.bb * 4 || ctx.pot_odds >= 0.25)))
            && (ctx.is_four_flush_board
                || (!allow_free_station_value && ctx.is_three_flush && ctx.is_paired_board))
            && self.is_guard_enabled(GuardId::PostPairedFlushStraightCap)
        {
            reasons.insert(GuardId::PostPairedFlushStraightCap);
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

    fn cap_idiot_end_straight(probs: &mut [f64], ctx: &GuardContext<'_>, reasons: &mut GuardSet) {
        reasons.insert(GuardId::PostPairedFlushStraightCap);
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
        reasons: &mut GuardSet,
    ) {
        reasons.insert(GuardId::PostPairedFlushStraightCap);
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
        if ctx.to_call > 0
            && (ctx.post_bucket.as_u8() < PostflopBucket::CLEAN_TWO_PAIR_MIN
                || ctx.to_call >= ctx.bb * 4)
        {
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
        reasons: &mut GuardSet,
    ) {
        if !ctx.is_three_flush
            || ctx.to_call <= 0
            || ctx.pot_odds < 0.16
            || ctx.bucket >= BUCKET_FLUSH_MIN
            || ctx.post_bucket.has_trips_flush_redraw()
            || !self.is_guard_enabled(GuardId::ThreeFlushNoRaise)
        {
            return;
        }
        let shifted_raise = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
        if shifted_raise > 0.0 {
            reasons.insert(GuardId::ThreeFlushNoRaise);
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += shifted_raise * 0.70;
            }
        }
    }
}
