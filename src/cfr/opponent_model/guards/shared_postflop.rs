/*! Shared Postflop Guard Rules Used Across Subgame, Blueprint Caps, and Tier-3 Barrel Defense */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::guard_id::{GuardId, GuardSet};
use super::helpers::{action_idx, first_action_idx, force_action, zero_and_sum};
use crate::cfr::abstraction::{BUCKET_STRAIGHT_MIN, BUCKET_WEAK_PAIR_MIN, PostflopBucket};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, ROUND_RIVER, ROUND_TURN,
};

/** Overpriced turn air / weak-draw call condition shared by subgame and blueprint guards. */
#[inline]
pub(in crate::cfr::opponent_model) fn is_turn_overpriced_air(ctx: &GuardContext<'_>) -> bool {
    let pb = ctx.post_bucket;
    ctx.round == ROUND_TURN
        && ctx.to_call >= ctx.bb
        && ((ctx.pot_odds >= 0.16 && pb.is_air_or_weak_draw())
            || (ctx.pot_odds > 0.12 && (pb.is_high_card() || pb.is_crushed_pair())))
}

/** Shared `TURN_DRAW_ODDS_FOLD` guard across both subgame and blueprint postflop pipelines. */
pub(in crate::cfr::opponent_model) fn apply_shared_turn_draw_odds_fold(
    tracker: &OpponentTracker,
    probs: &mut [f64],
    ctx: &GuardContext<'_>,
    reasons: &mut GuardSet,
) {
    let pb = ctx.post_bucket;
    if ctx.round != ROUND_TURN
        || ctx.to_call < ctx.bb * 2
        || !pb.is_strong_draw()
        || (ctx.is_overbet_bluffer && pb.is_nut_or_combo_draw() && ctx.pot_odds <= 0.36)
        || !tracker.is_guard_enabled(GuardId::TurnDrawOddsFold)
    {
        return;
    }
    let max_turn_draw_odds = if pb.is_nut_combo_or_pair_nfd() {
        0.40
    } else if pb.is_combo_draw() {
        0.32
    } else if pb.is_nut_or_combo_draw() {
        if ctx.is_overbet_bluffer { 0.36 } else { 0.30 }
    } else if pb.as_u8() >= 19 {
        if ctx.is_overbet_bluffer { 0.32 } else { 0.26 }
    } else if ctx.is_overbet_bluffer {
        0.25
    } else {
        0.19
    };
    if ctx.pot_odds > max_turn_draw_odds && force_action(probs, ctx.legal_actions, FOLD) {
        reasons.insert(GuardId::TurnDrawOddsFold);
    }
}

/** Shared Turn/River free pot-control and weak-pair overbet-strip rules (`to_call == 0`). */
pub(in crate::cfr::opponent_model) fn apply_shared_turn_river_free_pot_control(
    tracker: &OpponentTracker,
    probs: &mut [f64],
    ctx: &GuardContext<'_>,
) {
    if ctx.round < ROUND_TURN
        || ctx.to_call != 0
        || !tracker.is_guard_enabled(GuardId::PostPotControlCap)
    {
        return;
    }
    let pb = ctx.post_bucket;
    if ctx.is_four_board_danger && ctx.bucket < BUCKET_STRAIGHT_MIN {
        let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
        if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
            probs[c_i] = (probs[c_i] + shifted).max(0.95);
        }
    } else if (pb.is_air_or_weak_draw() || (ctx.round == ROUND_RIVER && pb.is_unpaired()))
        && !(ctx.from_subgame && ctx.has_river_nut_blocker)
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
            && pb.is_pure_rag_air();
        let allow_clean_hu_turn_barrel = ctx.round == ROUND_TURN
            && ctx.active_opponents == 1
            && !ctx.is_passive_station
            && !ctx.is_sticky_caller
            && !ctx.is_three_flush
            && !ctx.is_four_board_danger;
        let air_pot_cap = if allow_clean_hu_river_stab {
            ctx.bb * 16
        } else if allow_clean_hu_turn_barrel {
            ctx.bb * 14
        } else if pb.is_high_card() {
            ctx.bb * 8
        } else {
            ctx.bb * 18
        };
        let should_cap_bluff = (ctx.is_passive_station
            || ctx.is_sticky_caller
            || ctx.active_opponents >= 2
            || ctx.pot >= air_pot_cap
            || (ctx.is_three_flush && pb.is_air_or_weak_draw()))
            && !ctx.has_river_nut_blocker;
        if should_cap_bluff {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        }
    } else if ctx.round == ROUND_TURN
        && !ctx.from_subgame
        && pb.is_strong_draw()
        && (ctx.is_passive_station || ctx.active_opponents >= 2)
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
    } else if pb.is_weak_or_mid_pair() {
        if !ctx.is_passive_station {
            let overbet_w = zero_and_sum(probs, ctx.legal_actions, |a| {
                a == RAISE_HALF_POT
                    || (a == ALL_IN && (ctx.my_chips == 0 || ctx.my_chips > ctx.pot))
            });
            if overbet_w > 0.0
                && let Some(t_i) =
                    first_action_idx(ctx.legal_actions, &[RAISE_MIN, RAISE_THIRD_POT, CALL_CHECK])
            {
                probs[t_i] += overbet_w;
            }
        }
        let danger_pair_pot_cap = if ctx.from_subgame {
            ctx.bb * 5
        } else {
            ctx.bb * 8
        };
        if pb.in_range(BUCKET_WEAK_PAIR_MIN as u8, PostflopBucket::MID_PAIR_REDRAW)
            && !(ctx.is_passive_station
                && pb.as_u8() >= PostflopBucket::CLEAN_MID_PAIR_MIN
                && !ctx.is_three_flush)
            && ((ctx.round == ROUND_RIVER
                && ctx.pot >= danger_pair_pot_cap
                && (pb.as_u8() <= PostflopBucket::BOTTOM_PAIR_REDRAW
                    || ctx.is_paired_board
                    || ctx.is_three_flush
                    || ctx.is_four_board_danger))
                || (ctx.round == ROUND_TURN
                    && (ctx.pot >= ctx.bb * 12
                        || (pb.as_u8() <= PostflopBucket::BOTTOM_PAIR_REDRAW
                            && ctx.pot >= ctx.bb * 8)
                        || (ctx.pot >= danger_pair_pot_cap
                            && (ctx.is_paired_board
                                || ctx.is_three_flush
                                || ctx.is_four_board_danger)))))
        {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        }
    }
}

/** Paired or 3-flush Turn weak underpair fold shared by subgame and Tier-3 barrel defense. */
#[inline]
pub(in crate::cfr::opponent_model) fn is_turn_paired_or_flush_underpair_fold(
    ctx: &GuardContext<'_>,
) -> bool {
    ctx.round == ROUND_TURN
        && ctx.post_bucket.is_bare_weak_underpair()
        && !ctx.is_overbet_bluffer
        && ((ctx.is_paired_board
            && ctx.is_three_flush
            && ctx.to_call * 2 >= ctx.bb * 7
            && ctx.pot_odds >= 0.24)
            || ((ctx.is_paired_board || ctx.is_three_flush)
                && ctx.to_call * 2 >= ctx.bb * 7
                && ctx.pot_odds >= 0.25
                && (ctx.to_call >= ctx.bb * 5
                    || (ctx.my_chips > 0 && ctx.to_call * 5 >= ctx.my_chips * 2))))
}

/** Heavy commit-size Turn bare-underpair fold shared by subgame and Tier-3 barrel defense. */
#[inline]
pub(in crate::cfr::opponent_model) fn is_turn_commit_underpair_fold(
    ctx: &GuardContext<'_>,
) -> bool {
    ctx.round == ROUND_TURN
        && ctx.post_bucket.is_bare_weak_or_mid_pair()
        && !ctx.is_overbet_bluffer
        && !ctx.is_known_maniac
        && ctx.to_call >= ctx.bb * 15
        && ctx.pot_odds >= 0.25
        && ctx.my_chips > 0
        && ctx.to_call * 2 >= ctx.my_chips
}
