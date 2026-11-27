/*! Shared Postflop Guard Rules Used Across Subgame, Blueprint Caps, and Tier-3 Barrel Defense */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::guard_id::{GuardId, GuardSet};
use super::helpers::{action_idx, first_action_idx, force_action, zero_and_sum};
use crate::cfr::abstraction::{
    BUCKET_FLUSH_MIN, BUCKET_SECOND_NUT_FLUSH, BUCKET_STRAIGHT_MIN, BUCKET_TRIPS_MIN,
    BUCKET_WEAK_PAIR_MIN, PostflopBucket, detect_draws_detailed,
};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, ROUND_RIVER, ROUND_TURN,
};
use crate::game::holdem::Card;

/** Returns true when the board contains at least 3 distinct ranks strictly above hero's highest hole card. */
#[inline]
pub(in crate::cfr::opponent_model) fn has_three_plus_overcards(
    hole: Option<&[Card; 2]>,
    board: Option<&[Card]>,
) -> bool {
    let (Some(h), Some(b)) = (hole, board) else {
        return false;
    };
    let max_hole = (h[0].rank as usize).max(h[1].rank as usize);
    let mut over_mask = 0u16;
    for c in b {
        let r = c.rank as usize;
        if r > max_hole {
            over_mask |= 1u16 << r;
        }
    }
    over_mask.count_ones() >= 3
}

/** Returns true when hero holds an unpaired single pair (non-pocket) on an unpaired board with `>= 2` overcards. */
#[inline]
pub(in crate::cfr::opponent_model) fn is_unpaired_third_or_worse_pair(
    hole: Option<&[Card; 2]>,
    board: Option<&[Card]>,
    is_paired_board: bool,
) -> bool {
    if is_paired_board {
        return false;
    }
    let (Some(h), Some(b)) = (hole, board) else {
        return false;
    };
    if h[0].rank == h[1].rank {
        return false;
    }
    let mut board_mask = 0u16;
    for c in b {
        board_mask |= 1u16 << (c.rank as usize);
    }
    let r0 = h[0].rank as usize;
    let r1 = h[1].rank as usize;
    let hit0 = (board_mask & (1u16 << r0)) != 0;
    let hit1 = (board_mask & (1u16 << r1)) != 0;
    if hit0 == hit1 {
        return false;
    }
    let pair_rank = if hit0 { r0 } else { r1 };
    let over_mask = board_mask >> (pair_rank + 1);
    over_mask.count_ones() >= 2
}

/** Overpriced turn air / weak-draw call condition shared by subgame and blueprint guards. */
#[inline]
pub(in crate::cfr::opponent_model) fn is_turn_overpriced_air(ctx: &GuardContext<'_>) -> bool {
    let pb = ctx.post_bucket;
    if ctx.round != ROUND_TURN || ctx.to_call < ctx.bb {
        return false;
    }
    if pb.is_crushed_pair()
        && ctx.hole.zip(ctx.board).is_some_and(|(h, b)| {
            let d = detect_draws_detailed(h, b);
            d.is_straight_draw || d.is_flush_draw
        })
    {
        return false;
    }
    let strong_high_or_gutshot = (pb.as_u8() >= PostflopBucket::STRONG_ACE_KING_MIN
        && pb.is_high_card())
        || (pb.as_u8() >= 13 && pb.is_air_or_weak_draw());
    if strong_high_or_gutshot {
        ctx.pot_odds >= 0.22
    } else {
        (ctx.pot_odds >= 0.16 && pb.is_air_or_weak_draw())
            || (ctx.pot_odds > 0.12 && (pb.is_high_card() || pb.is_crushed_pair()))
    }
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
        if ctx.is_overbet_bluffer { 0.33 } else { 0.28 }
    } else if ctx.is_overbet_bluffer {
        0.27
    } else {
        0.23
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
    if ctx.is_four_flush_board && ctx.bucket <= BUCKET_SECOND_NUT_FLUSH {
        let oversized = zero_and_sum(probs, ctx.legal_actions, |a| {
            a == RAISE_HALF_POT || a == ALL_IN || (a >= RAISE_MIN && ctx.bucket < BUCKET_FLUSH_MIN)
        });
        if oversized > 0.0 {
            let target = if ctx.bucket >= BUCKET_FLUSH_MIN {
                first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK])
            } else {
                action_idx(ctx.legal_actions, CALL_CHECK)
            };
            if let Some(t_i) = target {
                probs[t_i] += oversized;
            }
        }
    }
    if ctx.round == ROUND_TURN
        && ctx.is_three_flush
        && ctx.bucket >= BUCKET_WEAK_PAIR_MIN
        && ctx.bucket < BUCKET_TRIPS_MIN
    {
        let oversized = zero_and_sum(probs, ctx.legal_actions, |a| {
            a == RAISE_HALF_POT || a == ALL_IN
        });
        if oversized > 0.0
            && let Some(t_i) =
                first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK])
        {
            probs[t_i] += oversized;
        }
    }
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
            && !pb.is_high_card()
            && !ctx.is_passive_station
            && !ctx.is_sticky_caller
            && !ctx.is_three_flush
            && !ctx.is_four_board_danger;
        let air_pot_cap = if allow_clean_hu_river_stab {
            ctx.bb * 16
        } else if allow_clean_hu_turn_barrel {
            ctx.bb * 10
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
