/*!
 * Extended postflop abstraction buckets (`88..=111`) adding O(1) river/turn
 * nut-blocker awareness, top-two/top-set/top-full-house separation, and flop/turn
 * potential-aware splits while preserving `0..=87` backward compatibility.
 */

use crate::game::config::{NUM_HOLE_CARDS, NUM_SUITS};
use crate::game::holdem::{Card, evaluate_7cards};

use super::super::draws::{detect_draws_detailed, is_double_gutshot};
use super::super::texture::{FastBoardSummary, compute_board_summary};
use super::high_card::detect_flop_backdoors;
use super::{find_unseen_flush_rank, postflop_equity_bucket_with_score};

pub const BUCKET_EXTENDED_MIN: usize = 88;
pub const BUCKET_RIVER_NUT_FLUSH_BLOCKER_AIR: usize = 88;
pub const BUCKET_RIVER_SECOND_NUT_FLUSH_BLOCKER_AIR: usize = 89;
pub const BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_AIR: usize = 90;
pub const BUCKET_RIVER_NUT_FLUSH_BLOCKER_WEAK_PAIR: usize = 91;
pub const BUCKET_RIVER_NUT_FLUSH_BLOCKER_TOP_PAIR: usize = 92;
pub const BUCKET_RIVER_NUT_FLUSH_BLOCKER_TWO_PAIR_PLUS: usize = 93;
pub const BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_TOP_PAIR: usize = 94;
pub const BUCKET_RIVER_STRAIGHT_NUT_FLUSH_BLOCKER: usize = 95;
pub const BUCKET_TOP_FULL_HOUSE: usize = 96;
pub const BUCKET_PAIRED_BOARD_NON_NUT_FLUSH: usize = 97;
pub const BUCKET_FLOP_TWO_OVERCARDS_AIR: usize = 98;
pub const BUCKET_FLOP_WEAK_PAIR_BDFD: usize = 99;
pub const BUCKET_FLOP_TOP_PAIR_BDFD: usize = 100;
pub const BUCKET_FLOP_OVERPAIR_BDFD: usize = 101;
pub const BUCKET_NFD_TWO_OVERCARDS: usize = 102;
pub const BUCKET_FD_TWO_OVERCARDS: usize = 103;
pub const BUCKET_PAIR_GUTSHOT: usize = 104;
pub const BUCKET_LOW_BOARD_TOP_PAIR: usize = 105;
pub const BUCKET_STRICT_TOP_TWO_PAIR: usize = 106;
pub const BUCKET_TRUE_TOP_SET: usize = 107;
pub const BUCKET_DOUBLE_GUTSHOT: usize = 108;
pub const BUCKET_TOP_PAIR_NUT_FD: usize = 109;
pub const BUCKET_PAIRED_BOARD_NUT_FLUSH: usize = 110;
pub const BUCKET_THREE_FLUSH_STRAIGHT: usize = 111;
pub const BUCKET_EXTENDED_MAX: usize = 111;

/** Returns `2` if holding the 1st-nut flush card on a 3-flush river,
 * `1` if holding the 2nd-nut flush card, or `0` otherwise. */
#[inline]
fn river_flush_blocker_level(hole: [Card; NUM_HOLE_CARDS], board: &[Card]) -> u8 {
    let mut suit_counts = [0u8; NUM_SUITS];
    for c in board {
        suit_counts[c.suit as usize] += 1;
    }
    let Some((flush_suit, _)) = suit_counts.iter().enumerate().find(|&(_, &cnt)| cnt == 3) else {
        return 0;
    };
    let nut_rank = find_unseen_flush_rank(board, flush_suit, 12);
    let second_nut_rank = find_unseen_flush_rank(board, flush_suit, nut_rank.saturating_sub(1));

    let mut level = 0u8;
    for c in hole {
        if c.suit as usize == flush_suit {
            let r = c.rank as usize;
            if r == nut_rank {
                return 2;
            }
            if r == second_nut_rank {
                level = 1;
            }
        }
    }
    level
}

/** Checks if `hole` blocks the highest 3+ card straight window on a river board. */
#[inline]
fn river_nut_straight_blocker(hole: [Card; NUM_HOLE_CARDS], board: &[Card]) -> bool {
    let mut board_mask = 0u16;
    for c in board {
        board_mask |= 1u16 << (c.rank as usize);
    }
    let hole_mask = (1u16 << (hole[0].rank as usize)) | (1u16 << (hole[1].rank as usize));

    for start in (0..=8usize).rev() {
        let win_mask = 0x1Fu16 << start;
        if (board_mask & win_mask).count_ones() >= 3 {
            let missing = win_mask & !board_mask;
            return (hole_mask & missing) != 0;
        }
    }
    if (board_mask & 0x100Fu16).count_ones() >= 3 {
        let missing = 0x100Fu16 & !board_mask;
        return (hole_mask & missing) != 0;
    }
    false
}

#[inline]
fn is_strict_top_two_pair(hole: [Card; NUM_HOLE_CARDS], board: &[Card]) -> bool {
    let mut r_top = None;
    let mut r_second = None;
    for r in (0..13usize).rev() {
        if board.iter().any(|c| c.rank as usize == r) {
            if r_top.is_none() {
                r_top = Some(r);
            } else if r_second.is_none() {
                r_second = Some(r);
                break;
            }
        }
    }
    let (Some(top), Some(sec)) = (r_top, r_second) else {
        return false;
    };
    let h0 = hole[0].rank as usize;
    let h1 = hole[1].rank as usize;
    (h0 == top && h1 == sec) || (h0 == sec && h1 == top)
}

#[inline]
fn bucket_extended_river(
    category: u64,
    b0: usize,
    hole: [Card; NUM_HOLE_CARDS],
    board: &[Card],
    summary: FastBoardSummary,
) -> usize {
    let fl_blocker = if category < 5 {
        river_flush_blocker_level(hole, board)
    } else {
        0
    };
    if fl_blocker == 2 {
        if (74..=78).contains(&b0) {
            return BUCKET_RIVER_STRAIGHT_NUT_FLUSH_BLOCKER;
        }
        if (57..=72).contains(&b0) {
            return BUCKET_RIVER_NUT_FLUSH_BLOCKER_TWO_PAIR_PLUS;
        }
        if (40..=56).contains(&b0) {
            return BUCKET_RIVER_NUT_FLUSH_BLOCKER_TOP_PAIR;
        }
        if (28..=39).contains(&b0) {
            return BUCKET_RIVER_NUT_FLUSH_BLOCKER_WEAK_PAIR;
        }
        if b0 <= 10 {
            return BUCKET_RIVER_NUT_FLUSH_BLOCKER_AIR;
        }
    }
    if fl_blocker == 1 && b0 <= 10 {
        return BUCKET_RIVER_SECOND_NUT_FLUSH_BLOCKER_AIR;
    }
    if category < 4 && !summary.is_three_fl && river_nut_straight_blocker(hole, board) {
        if (40..=56).contains(&b0) {
            return BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_TOP_PAIR;
        }
        if b0 <= 10 {
            return BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_AIR;
        }
    }
    if category == 4 && summary.is_three_fl && (75..=78).contains(&b0) {
        return BUCKET_THREE_FLUSH_STRAIGHT;
    }
    b0
}

#[inline]
fn bucket_extended_flop_turn(
    category: u64,
    score: u64,
    b0: usize,
    hole: [Card; NUM_HOLE_CARDS],
    board: &[Card],
    summary: FastBoardSummary,
) -> usize {
    if category == 4 && summary.is_three_fl && (75..=78).contains(&b0) {
        return BUCKET_THREE_FLUSH_STRAIGHT;
    }

    let draws = detect_draws_detailed(&hole, board);
    if b0 == 49 && draws.is_nut_flush_draw {
        return BUCKET_TOP_PAIR_NUT_FD;
    }
    if (11..=14).contains(&b0) && is_double_gutshot(&hole, board) {
        return BUCKET_DOUBLE_GUTSHOT;
    }

    let h0 = hole[0].rank as usize;
    let h1 = hole[1].rank as usize;
    let two_overcards = h0 > summary.max_rank && h1 > summary.max_rank;

    if (22..=23).contains(&b0) && two_overcards {
        return BUCKET_NFD_TWO_OVERCARDS;
    }
    if (19..=21).contains(&b0) && two_overcards {
        return BUCKET_FD_TWO_OVERCARDS;
    }
    if (28..=39).contains(&b0)
        && draws.is_gutshot
        && !draws.is_flush_draw
        && !summary.is_four_fl
        && !summary.is_four_st
    {
        return BUCKET_PAIR_GUTSHOT;
    }

    if board.len() == 3 {
        let (_, nut_bdfd, _) = detect_flop_backdoors(hole, board);
        if nut_bdfd {
            if (51..=55).contains(&b0) {
                return BUCKET_FLOP_OVERPAIR_BDFD;
            }
            if (41..=47).contains(&b0) {
                return BUCKET_FLOP_TOP_PAIR_BDFD;
            }
            if (28..=39).contains(&b0) {
                return BUCKET_FLOP_WEAK_PAIR_BDFD;
            }
        }
        if b0 <= 10 && two_overcards {
            return BUCKET_FLOP_TWO_OVERCARDS_AIR;
        }
    }

    if category == 1 && (44..=47).contains(&b0) {
        let pair_rank = ((score >> 16) & 0xF) as usize;
        if pair_rank <= 9 {
            return BUCKET_LOW_BOARD_TOP_PAIR;
        }
    }

    b0
}

/** Extended postflop bucketing (`0..=111`) that splits `88..=111` blocker and potential
 * scenarios out of `0..=87` without modifying any base `0..=87` definitions. */
#[inline]
pub fn postflop_extended_bucket(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> usize {
    postflop_extended_bucket_with_score(hole, board, evaluate_7cards(hole, board))
}

#[inline]
pub fn postflop_extended_bucket_with_score(
    hole: &[Card; NUM_HOLE_CARDS],
    board: &[Card],
    score: u64,
) -> usize {
    let b0 = postflop_equity_bucket_with_score(hole, board, score);
    let category = score >> 32;
    let summary = compute_board_summary(board);

    if category == 6 && b0 == 85 {
        let trips_rank = ((score >> 16) & 0xF) as usize;
        let pair_rank = (score & 0xF) as usize;
        if trips_rank >= summary.max_rank || pair_rank > summary.max_rank {
            return BUCKET_TOP_FULL_HOUSE;
        }
    }
    if category == 5 && summary.is_paired {
        if (82..=83).contains(&b0) {
            return BUCKET_PAIRED_BOARD_NUT_FLUSH;
        }
        if (79..=81).contains(&b0) {
            return BUCKET_PAIRED_BOARD_NON_NUT_FLUSH;
        }
    }
    if category == 3 && (69..=71).contains(&b0) && hole[0].rank == hole[1].rank {
        let trips_rank = ((score >> 16) & 0xF) as usize;
        if trips_rank >= summary.max_rank {
            return BUCKET_TRUE_TOP_SET;
        }
    }
    if category == 2 && (61..=63).contains(&b0) && is_strict_top_two_pair(*hole, board) {
        return BUCKET_STRICT_TOP_TWO_PAIR;
    }

    if board.len() >= 5 {
        bucket_extended_river(category, b0, *hole, board, summary)
    } else {
        bucket_extended_flop_turn(category, score, b0, *hole, board, summary)
    }
}
