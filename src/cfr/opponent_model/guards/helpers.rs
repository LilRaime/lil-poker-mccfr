/*! Shared Zero-Cost Guard Action Helpers & Cross-Tier Safety Invariants */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use crate::game::config::{ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT};
use crate::game::holdem::Card;

#[inline]
pub(in crate::cfr::opponent_model) fn action_idx(legal: &[u8], action: u8) -> Option<usize> {
    legal.iter().position(|&a| a == action)
}

#[inline]
pub(in crate::cfr::opponent_model) fn first_action_idx(
    legal: &[u8],
    candidates: &[u8],
) -> Option<usize> {
    for &c in candidates {
        if let Some(idx) = action_idx(legal, c) {
            return Some(idx);
        }
    }
    None
}

#[inline]
pub(in crate::cfr::opponent_model) fn force_action(
    probs: &mut [f64],
    legal: &[u8],
    action: u8,
) -> bool {
    let Some(idx) = action_idx(legal, action) else {
        return false;
    };
    for p in probs.iter_mut() {
        *p = 0.0;
    }
    probs[idx] = 1.0;
    true
}

#[inline]
pub(in crate::cfr::opponent_model) fn zero_and_sum(
    probs: &mut [f64],
    legal: &[u8],
    pred: impl Fn(u8) -> bool,
) -> f64 {
    let mut shifted = 0.0f64;
    for (i, &a) in legal.iter().enumerate() {
        if pred(a) {
            shifted += probs[i];
            probs[i] = 0.0;
        }
    }
    shifted
}

#[inline]
pub(super) fn shift_action_to(probs: &mut [f64], legal: &[u8], from: u8, targets: &[u8]) -> f64 {
    let Some(f_i) = action_idx(legal, from) else {
        return 0.0;
    };
    let removed = probs[f_i];
    if removed <= 0.0 {
        return 0.0;
    }
    probs[f_i] = 0.0;
    if let Some(t_i) = first_action_idx(legal, targets) {
        probs[t_i] += removed;
    }
    removed
}

#[inline]
pub(super) fn river_nut_blocker_allowed(pot: i32, bb: i32, my_chips: i32) -> bool {
    pot <= bb * 12 || (my_chips > 0 && my_chips * 4 <= pot * 3)
}

#[inline]
pub(super) fn is_three_straight_board(board: &[Card]) -> bool {
    let mut ranks = [false; 13];
    for c in board {
        ranks[c.rank as usize] = true;
    }
    (0..=9).any(|s| (s..s + 4).filter(|&r| ranks[r]).count() >= 3)
}

/** Cap extreme > 3x pot ALL_IN shoves for ALL hands when effective stack is known */
pub(super) fn apply_overbet_allin_cap(
    tracker: &OpponentTracker,
    probs: &mut [f64],
    ctx: &GuardContext<'_>,
    reasons: &mut Vec<&'static str>,
) {
    if ctx.my_chips <= 0 || ctx.is_allin_call || !tracker.is_guard_enabled("POST_OVERBET_ALLIN_CAP")
    {
        return;
    }
    let rem_after_call = (ctx.my_chips - ctx.to_call).max(0);
    let pot_after_call = (ctx.pot + ctx.to_call).max(1);
    if rem_after_call <= pot_after_call * 3 {
        return;
    }
    let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN) else {
        return;
    };
    let removed = probs[ai_i];
    if removed <= 0.0 {
        return;
    }
    reasons.push("POST_OVERBET_ALLIN_CAP");
    probs[ai_i] = 0.0;
    let target_idx = if ctx.to_call == 0 {
        if ctx.bucket < 65
            && (ctx.is_paired_board || ctx.is_three_flush || ctx.is_four_board_danger)
        {
            first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, CALL_CHECK])
        } else {
            first_action_idx(
                ctx.legal_actions,
                &[RAISE_HALF_POT, RAISE_THIRD_POT, CALL_CHECK],
            )
        }
    } else if ctx.bucket >= 65 && !ctx.is_four_board_danger {
        first_action_idx(
            ctx.legal_actions,
            &[RAISE_HALF_POT, RAISE_THIRD_POT, CALL_CHECK],
        )
    } else {
        action_idx(ctx.legal_actions, CALL_CHECK)
    };
    if let Some(t_i) = target_idx {
        probs[t_i] += removed;
    }
}

/** River bluff discipline: no A/K-high showdown-value bets, no 25% micro-bluff into paired
 * rivers, and no 125% overbet on paired river with weak/medium pairs (28..=43). */
pub(super) fn apply_river_bluff_cap(
    tracker: &OpponentTracker,
    probs: &mut [f64],
    ctx: &GuardContext<'_>,
    strip_half_and_allin_for_air: bool,
    reasons: &mut Vec<&'static str>,
) {
    if ctx.round != 4 || ctx.to_call != 0 || !tracker.is_guard_enabled("POST_RIVER_BLUFF_CAP") {
        return;
    }
    if ctx.bucket < 28 {
        if !strip_half_and_allin_for_air && ctx.has_river_nut_blocker {
            return;
        }
        let shifted = zero_and_sum(probs, ctx.legal_actions, |a| {
            (strip_half_and_allin_for_air && (a == ALL_IN || a == RAISE_HALF_POT))
                || ((8..=10).contains(&ctx.bucket) && !ctx.has_river_nut_blocker && a >= RAISE_MIN)
                || (ctx.is_paired_board
                    && ctx.bucket < 11
                    && !ctx.has_river_nut_blocker
                    && a == RAISE_MIN)
        });
        if shifted > 0.0 {
            reasons.push("POST_RIVER_BLUFF_CAP");
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += shifted;
            }
        }
    } else if (28..=44).contains(&ctx.bucket)
        || ((45..=66).contains(&ctx.bucket)
            && (ctx.is_paired_board
                || ctx.is_dangerous_board
                || ctx.is_three_flush
                || ctx.is_three_straight_board))
    {
        let shifted = zero_and_sum(probs, ctx.legal_actions, |a| {
            a == RAISE_HALF_POT || a == ALL_IN
        });
        if shifted > 0.0 {
            reasons.push("POST_RIVER_BLUFF_CAP");
            let c_idx = action_idx(ctx.legal_actions, CALL_CHECK);
            let tp_idx = action_idx(ctx.legal_actions, RAISE_THIRD_POT);
            if let (Some(c_i), Some(tp_i)) = (c_idx, tp_idx) {
                probs[c_i] += shifted * 0.70;
                probs[tp_i] += shifted * 0.30;
            } else if let Some(c_i) = c_idx {
                probs[c_i] += shifted;
            }
        }
    }
}

#[inline]
pub(super) fn apply_pot_commit_terminal(
    tracker: &OpponentTracker,
    probs: &mut [f64],
    ctx: &GuardContext<'_>,
    reasons: &mut Vec<&'static str>,
) {
    if ctx.to_call <= 0
        || !(ctx.pot_odds <= 0.12 || ctx.never_fold_postflop)
        || !tracker.is_guard_enabled("POT_COMMIT_TERMINAL")
    {
        return;
    }
    if shift_action_to(probs, ctx.legal_actions, FOLD, &[CALL_CHECK, ALL_IN]) > 0.0 {
        reasons.push("POT_COMMIT_TERMINAL");
    }
}
