/*! Two-Pair, Paired-Board One-Pair, and Unpaired-Board One-Pair Bucketing */
use crate::game::config::NUM_HOLE_CARDS;
use crate::game::holdem::Card;

use super::super::draws::{DrawInfo, detect_draws_detailed};
use super::super::texture::{FastBoardSummary, compute_board_summary};
use super::high_card::bucket_high_card_or_draw;

#[derive(Clone, Copy)]
struct PairBoardFlags {
    board_len: usize,
    is_four_fl: bool,
    is_four_st: bool,
    is_three_fl: bool,
    is_vulnerable: bool,
}

impl PairBoardFlags {
    #[inline(always)]
    fn from_summary(s: FastBoardSummary) -> Self {
        Self {
            board_len: s.board_len,
            is_four_fl: s.is_four_fl,
            is_four_st: s.is_four_st,
            is_three_fl: s.is_three_fl,
            is_vulnerable: s.is_three_fl || s.is_dangerous || s.is_paired,
        }
    }
}

pub(super) fn bucket_two_pair_or_paired_board(
    score: u64,
    hole: [Card; NUM_HOLE_CARDS],
    board: &[Card],
) -> usize {
    let top_pair = ((score >> 16) & 0xF) as usize;
    let bottom_pair = ((score >> 8) & 0xF) as usize;
    let is_pocket_pair = hole[0].rank == hole[1].rank;
    let hit_top = (hole[0].rank as usize == top_pair) || (hole[1].rank as usize == top_pair);
    let hit_bottom =
        (hole[0].rank as usize == bottom_pair) || (hole[1].rank as usize == bottom_pair);

    if !hit_top && !hit_bottom {
        bucket_board_two_pair(hole, board, top_pair, bottom_pair)
    } else if !is_pocket_pair && hit_top && hit_bottom {
        bucket_genuine_two_pair(hole, board, top_pair)
    } else {
        bucket_paired_board_one_pair(hole, board, top_pair, bottom_pair, is_pocket_pair, hit_top)
    }
}

fn bucket_board_two_pair(
    hole: [Card; NUM_HOLE_CARDS],
    board: &[Card],
    top_pair: usize,
    bottom_pair: usize,
) -> usize {
    if board.len() < 5 {
        let draw_bucket = bucket_high_card_or_draw(hole, board);
        if draw_bucket >= 11 {
            return draw_bucket;
        }
    }
    let high_kicker = (hole[0].rank as usize).max(hole[1].rank as usize);
    let low_kicker = (hole[0].rank as usize).min(hole[1].rank as usize);
    let max_unpaired_board = board
        .iter()
        .map(|c| c.rank as usize)
        .filter(|&r| r != top_pair && r != bottom_pair)
        .max()
        .unwrap_or(0);
    if high_kicker > max_unpaired_board && high_kicker == 12 {
        if low_kicker >= 9 {
            10 /* Board Two Pair with Ace-Broadway kicker */
        } else {
            9 /* Board Two Pair with Ace-rag kicker */
        }
    } else if high_kicker > max_unpaired_board && high_kicker == 11 {
        8 /* Board Two Pair with King kicker */
    } else {
        high_kicker.min(7) /* Counterfeited/weak kicker on double-paired board */
    }
}

fn bucket_genuine_two_pair(hole: [Card; NUM_HOLE_CARDS], board: &[Card], top_pair: usize) -> usize {
    let draws = detect_draws_detailed(&hole, board);
    let overcards_count = board
        .iter()
        .filter(|c| (c.rank as usize) > top_pair)
        .count();
    let summary = compute_board_summary(board);
    let is_four_fl = summary.is_four_fl;
    let is_four_st = summary.is_four_st;
    let is_vulnerable = summary.is_three_fl || summary.is_dangerous;

    if board.len() < 5 && draws.is_flush_draw {
        64 /* Genuine Two Pair + Flush Redraw */
    } else if is_four_fl || is_four_st {
        57 /* Two Pair on 4-flush or 4-straight board without flush/straight */
    } else if overcards_count >= 2 {
        58 /* Two Pair under 2+ overcards */
    } else if overcards_count == 1 {
        59 /* Two Pair under 1 overcard */
    } else if is_vulnerable {
        60 /* Genuine Top Two Pair on vulnerable 3-flush / connected board */
    } else if top_pair >= 10 {
        63 /* High Genuine Top Two Pair */
    } else if top_pair >= 7 {
        62 /* Mid Genuine Top Two Pair */
    } else {
        61 /* Low Genuine Top Two Pair */
    }
}

fn resolve_paired_board_rank_and_kicker(
    hole: [Card; NUM_HOLE_CARDS],
    top_pair: usize,
    bottom_pair: usize,
    is_pocket_pair: bool,
    hit_top: bool,
) -> (usize, usize) {
    if is_pocket_pair {
        (hole[0].rank as usize, hole[0].rank as usize)
    } else if hit_top {
        let k = if hole[0].rank as usize == top_pair {
            hole[1].rank as usize
        } else {
            hole[0].rank as usize
        };
        (top_pair, k)
    } else {
        let k = if hole[0].rank as usize == bottom_pair {
            hole[1].rank as usize
        } else {
            hole[0].rank as usize
        };
        (bottom_pair, k)
    }
}

fn count_distinct_overcards(board: &[Card], pair_rank: usize) -> usize {
    let mut higher_rank_mask = 0u16;
    for c in board {
        let r = c.rank as usize;
        if r > pair_rank {
            higher_rank_mask |= 1 << r;
        }
    }
    higher_rank_mask.count_ones() as usize
}

fn bucket_paired_board_one_pair(
    hole: [Card; NUM_HOLE_CARDS],
    board: &[Card],
    top_pair: usize,
    bottom_pair: usize,
    is_pocket_pair: bool,
    hit_top: bool,
) -> usize {
    let draws = detect_draws_detailed(&hole, board);
    let summary = compute_board_summary(board);
    let flags = PairBoardFlags::from_summary(summary);
    let max_board_rank = summary.max_rank;
    let (my_pair_rank, kicker) =
        resolve_paired_board_rank_and_kicker(hole, top_pair, bottom_pair, is_pocket_pair, hit_top);

    if is_pocket_pair && my_pair_rank > max_board_rank {
        bucket_paired_overpair(my_pair_rank, draws, flags)
    } else if !is_pocket_pair && my_pair_rank >= max_board_rank {
        let max_unpaired_board = board
            .iter()
            .map(|c| c.rank as usize)
            .filter(|&r| r != top_pair && r != bottom_pair)
            .max()
            .unwrap_or(0);
        let kicker_counterfeited = board.len() >= 5 && kicker <= max_unpaired_board;
        bucket_paired_top_pair(kicker, kicker_counterfeited, draws, flags)
    } else {
        let distinct_overcards = count_distinct_overcards(board, my_pair_rank);
        if is_pocket_pair {
            bucket_paired_pocket_underpair(distinct_overcards, draws, flags)
        } else {
            bucket_paired_lower_pair(distinct_overcards, draws, flags)
        }
    }
}

fn bucket_paired_overpair(pair_rank: usize, draws: DrawInfo, f: PairBoardFlags) -> usize {
    if f.board_len < 5
        && ((draws.is_flush_draw && !f.is_four_fl)
            || (draws.is_straight_draw && !f.is_four_st && !f.is_four_fl))
    {
        56 /* Overpair + Flush/Straight Redraw */
    } else if f.is_four_fl || f.is_four_st {
        50 /* Overpair on 4-flush or 4-straight board */
    } else if pair_rank >= 11 {
        52 /* Vulnerable High Overpair (AA, KK) on paired board */
    } else {
        51 /* Vulnerable Low/Mid Overpair on paired board */
    }
}

fn bucket_paired_top_pair(
    kicker: usize,
    kicker_counterfeited: bool,
    draws: DrawInfo,
    f: PairBoardFlags,
) -> usize {
    if f.board_len < 5 && draws.is_flush_draw && !f.is_four_fl {
        49 /* Top Pair + Flush Redraw */
    } else if f.board_len < 5
        && (draws.is_straight_draw || draws.is_gutshot)
        && !f.is_four_st
        && !f.is_four_fl
    {
        48 /* Top Pair + Straight/Gutshot Redraw */
    } else if kicker_counterfeited {
        39 /* Top Pair with counterfeited kicker on paired river */
    } else if f.is_four_fl || f.is_four_st {
        40 /* Top Pair on 4-flush or 4-straight paired board */
    } else if kicker >= 11 {
        43 /* Vulnerable Top Pair Top Kicker on paired board */
    } else if kicker >= 6 {
        42 /* Vulnerable Top Pair Mid/Good Kicker on paired board */
    } else {
        41 /* Vulnerable Top Pair Weak Kicker on paired board */
    }
}

fn bucket_paired_pocket_underpair(
    distinct_overcards: usize,
    draws: DrawInfo,
    f: PairBoardFlags,
) -> usize {
    if f.board_len < 5 && draws.is_nut_flush_draw && !f.is_four_fl {
        26 /* Pocket Pair + Nut Flush Draw */
    } else if f.board_len < 5
        && ((draws.is_flush_draw && !f.is_four_fl)
            || (draws.is_straight_draw && !f.is_four_st && !f.is_four_fl))
    {
        38 /* Pocket Pair + Flush/Straight Draw */
    } else if f.is_four_fl || f.is_four_st {
        28 /* Pocket Pair on 4-flush / 4-straight paired board */
    } else if distinct_overcards >= 3 {
        32 /* Pocket Pair under 3+ overcards */
    } else if distinct_overcards == 2 {
        34 /* Pocket Pair under 2 overcards */
    } else {
        37 /* Pocket Pair under 1 overcard on paired board */
    }
}

fn bucket_paired_lower_pair(
    distinct_overcards: usize,
    draws: DrawInfo,
    f: PairBoardFlags,
) -> usize {
    if f.board_len < 5 && draws.is_nut_flush_draw && !f.is_four_fl {
        26 /* Pair + Nut Flush Draw */
    } else if f.board_len < 5
        && ((draws.is_flush_draw && !f.is_four_fl)
            || (draws.is_straight_draw && !f.is_four_st && !f.is_four_fl))
    {
        if distinct_overcards == 1 {
            38 /* Middle Pair + Draw */
        } else {
            33 /* Bottom Pair + Draw */
        }
    } else if f.is_four_fl || f.is_four_st {
        28 /* Weak Pair on 4-flush / 4-straight paired board */
    } else if distinct_overcards == 1 {
        34 /* Middle/2nd Pair on paired board */
    } else {
        29 /* Bottom/3rd+ Pair on paired board */
    }
}

pub(super) fn bucket_one_pair(score: u64, hole: [Card; NUM_HOLE_CARDS], board: &[Card]) -> usize {
    let pair_rank = ((score >> 16) & 0xF) as usize;
    let is_pocket_pair = hole[0].rank == hole[1].rank;
    let hole_hit_pair =
        (hole[0].rank as usize == pair_rank) || (hole[1].rank as usize == pair_rank);
    let draws = detect_draws_detailed(&hole, board);

    if !is_pocket_pair && !hole_hit_pair {
        return bucket_board_pair_high_card(hole, board, draws);
    }

    let summary = compute_board_summary(board);
    let max_board_rank = summary.max_rank;
    let min_board_rank = summary.min_rank;
    let flags = PairBoardFlags::from_summary(summary);

    if is_pocket_pair && pair_rank > max_board_rank {
        bucket_unpaired_overpair(pair_rank, draws, flags)
    } else if hole_hit_pair && pair_rank >= max_board_rank {
        bucket_unpaired_top_pair(hole, board, pair_rank, draws, flags)
    } else if pair_rank > min_board_rank {
        bucket_unpaired_middle_pair(hole, board, pair_rank, is_pocket_pair, draws, flags)
    } else {
        bucket_unpaired_bottom_pair(hole, pair_rank, is_pocket_pair, draws, flags)
    }
}

fn bucket_board_pair_high_card(
    hole: [Card; NUM_HOLE_CARDS],
    board: &[Card],
    draws: DrawInfo,
) -> usize {
    let high_kicker = (hole[0].rank as usize).max(hole[1].rank as usize);
    let low_kicker = (hole[0].rank as usize).min(hole[1].rank as usize);
    if board.len() < 5 && draws.is_flush_draw && draws.is_straight_draw {
        25 /* Combo draw on paired board */
    } else if board.len() < 5 && draws.is_nut_flush_draw {
        22 /* Nut Flush Draw on paired board */
    } else if board.len() < 5 && draws.is_flush_draw {
        20 /* Flush Draw on paired board */
    } else if board.len() < 5 && draws.is_straight_draw {
        16 /* OESD on paired board */
    } else if board.len() < 5 && draws.is_gutshot {
        if high_kicker >= 11 { 13 } else { 12 }
    } else if high_kicker == 12 && low_kicker >= 9 {
        10 /* Board pair with strong Ace-Broadway kicker */
    } else if high_kicker == 12 {
        9 /* Board pair with Ace-rag kicker */
    } else if high_kicker == 11 {
        8 /* Board pair with King kicker */
    } else {
        high_kicker.min(7) /* Board pair with Queen-high or worse (pure air) */
    }
}

fn bucket_unpaired_overpair(pair_rank: usize, draws: DrawInfo, f: PairBoardFlags) -> usize {
    if f.board_len < 5
        && ((draws.is_flush_draw && !f.is_four_fl)
            || (draws.is_straight_draw && !f.is_four_st && !f.is_four_fl))
    {
        56 /* Overpair + Flush/Straight Draw */
    } else if f.is_four_fl || f.is_four_st {
        50 /* Overpair on 4-flush or 4-straight board without redraw */
    } else if f.is_vulnerable {
        if pair_rank >= 11 {
            52 /* Vulnerable High Overpair: AA, KK */
        } else {
            51 /* Vulnerable Low/Mid Overpair: 88..QQ */
        }
    } else if pair_rank >= 11 {
        55 /* Dry High Overpair: AA, KK */
    } else if pair_rank >= 9 {
        54 /* Dry Mid Overpair: QQ, JJ */
    } else {
        53 /* Dry Low Overpair: TT, 99, 88 */
    }
}

fn bucket_unpaired_top_pair(
    hole: [Card; NUM_HOLE_CARDS],
    board: &[Card],
    pair_rank: usize,
    draws: DrawInfo,
    f: PairBoardFlags,
) -> usize {
    let kicker = if hole[0].rank as usize == pair_rank {
        hole[1].rank as usize
    } else {
        hole[0].rank as usize
    };
    let kicker_plays = board.iter().filter(|c| (c.rank as usize) > kicker).count() < 4;

    if f.board_len < 5 && draws.is_flush_draw && !f.is_four_fl {
        49 /* Top Pair + Flush Draw */
    } else if f.board_len < 5
        && (draws.is_straight_draw || (draws.is_gutshot && !f.is_vulnerable))
        && !f.is_four_st
        && !f.is_four_fl
        && !f.is_three_fl
    {
        48 /* Top Pair + OESD (or Dry Top Pair + Gutshot) */
    } else if !kicker_plays {
        39 /* Top Pair with completely counterfeited kicker */
    } else if f.is_four_fl || f.is_four_st {
        40 /* Top Pair on 4-flush or 4-straight board */
    } else if f.is_vulnerable {
        if kicker >= 11 {
            43 /* Vulnerable Top Pair Top Kicker */
        } else if kicker >= 6 {
            42 /* Vulnerable Top Pair Mid/Good Kicker */
        } else {
            41 /* Vulnerable Top Pair Weak Kicker */
        }
    } else if kicker >= 11 {
        47 /* Dry Top Pair Top Kicker */
    } else if kicker >= 8 {
        46 /* Dry Top Pair Good Kicker */
    } else if kicker >= 5 {
        45 /* Dry Top Pair Mid Kicker */
    } else {
        44 /* Dry Top Pair Weak Kicker */
    }
}

fn bucket_unpaired_middle_pair(
    hole: [Card; NUM_HOLE_CARDS],
    board: &[Card],
    pair_rank: usize,
    is_pocket_pair: bool,
    draws: DrawInfo,
    f: PairBoardFlags,
) -> usize {
    let overcards_count = board
        .iter()
        .filter(|c| (c.rank as usize) > pair_rank)
        .count();
    if f.board_len < 5 && draws.is_nut_flush_draw && !f.is_four_fl {
        26 /* Middle/Bottom Pair + Nut Flush Draw */
    } else if f.board_len < 5
        && ((draws.is_flush_draw && !f.is_four_fl)
            || (draws.is_straight_draw && !f.is_four_st && !f.is_four_fl))
    {
        38 /* Middle Pair + Draw */
    } else if f.is_four_fl || f.is_four_st {
        28 /* Counterfeited / crushed pair on 4-straight or 4-flush board */
    } else if is_pocket_pair {
        if overcards_count >= 3 {
            32 /* Pocket pair under 3+ board overcards */
        } else if overcards_count == 2 {
            34 /* Pocket pair under 2 board overcards */
        } else {
            37 /* Pocket pair under single overcard */
        }
    } else {
        let kicker = if hole[0].rank as usize == pair_rank {
            hole[1].rank as usize
        } else {
            hole[0].rank as usize
        };
        if overcards_count >= 3 && f.is_three_fl {
            28 /* Crushed 4th pair on 3-flush board */
        } else if overcards_count >= 3 {
            29 /* 4th pair on board is weak bottom pair */
        } else if f.is_three_fl {
            34 /* Middle Pair on 3-flush board */
        } else if kicker >= 10 && overcards_count == 1 {
            36 /* Dry Middle Pair, high kicker */
        } else {
            35 /* Dry Middle Pair, low/mid kicker */
        }
    }
}

fn bucket_unpaired_bottom_pair(
    hole: [Card; NUM_HOLE_CARDS],
    pair_rank: usize,
    is_pocket_pair: bool,
    draws: DrawInfo,
    f: PairBoardFlags,
) -> usize {
    if f.board_len < 5 && draws.is_nut_flush_draw && !f.is_four_fl {
        26 /* Bottom Pair + Nut Flush Draw */
    } else if f.board_len < 5
        && ((draws.is_flush_draw && !f.is_four_fl)
            || (draws.is_straight_draw && !f.is_four_st && !f.is_four_fl))
    {
        33 /* Bottom Pair + Draw */
    } else if f.is_four_st || f.is_four_fl {
        28 /* Bottom Pair / Underpair on 4-straight / 4-flush board */
    } else if is_pocket_pair {
        32 /* Underpair */
    } else if f.is_three_fl {
        29 /* Bottom Pair on 3-flush board */
    } else {
        let kicker = if hole[0].rank as usize == pair_rank {
            hole[1].rank as usize
        } else {
            hole[0].rank as usize
        };
        if kicker >= 11 {
            31 /* Dry Bottom Pair with A/K kicker */
        } else {
            30 /* Dry Bottom Pair with weak kicker */
        }
    }
}
