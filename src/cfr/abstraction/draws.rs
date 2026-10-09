/*
 * Fast O(1) draw detection for Flop & Turn (Flush draws, Nut Flush draws, Straight draws, Gutshots).
 */

use crate::game::holdem::{Card, Rank};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawInfo {
    pub is_flush_draw: bool,
    pub is_nut_flush_draw: bool,
    pub is_straight_draw: bool,
    pub is_gutshot: bool,
}

#[inline(always)]
pub fn detect_draws_detailed(hole: &[Card; 2], board: &[Card]) -> DrawInfo {
    if board.len() >= 5 {
        return DrawInfo {
            is_flush_draw: false,
            is_nut_flush_draw: false,
            is_straight_draw: false,
            is_gutshot: false,
        };
    }

    let mut board_suit_counts = [0u8; 4];
    for &b in board {
        board_suit_counts[b.suit as usize] += 1;
    }

    let mut is_flush_draw = false;
    let mut is_nut_flush_draw = false;
    for (s, &b_count) in board_suit_counts.iter().enumerate() {
        let hole_has = (hole[0].suit as usize == s) as u8 + (hole[1].suit as usize == s) as u8;
        if hole_has >= 1 && (b_count + hole_has == 4) {
            is_flush_draw = true;
            if (hole[0].rank == Rank::Ace && hole[0].suit as usize == s)
                || (hole[1].rank == Rank::Ace && hole[1].suit as usize == s)
            {
                is_nut_flush_draw = true;
            }
            break;
        }
    }

    let mut ranks = [false; 13];
    for &b in board {
        ranks[b.rank as usize] = true;
    }
    ranks[hole[0].rank as usize] = true;
    ranks[hole[1].rank as usize] = true;

    let h0 = hole[0].rank as usize;
    let h1 = hole[1].rank as usize;
    let mut is_straight_draw = false;
    for r in 0..10 {
        if ranks[r]
            && ranks[r + 1]
            && ranks[r + 2]
            && ranks[r + 3]
            && ((h0 >= r && h0 <= r + 3) || (h1 >= r && h1 <= r + 3))
        {
            is_straight_draw = true;
            break;
        }
    }
    if !is_straight_draw && ranks[12] {
        let low_count = (ranks[0] as u8) + (ranks[1] as u8) + (ranks[2] as u8) + (ranks[3] as u8);
        if low_count >= 3 && (h0 == 12 || h0 <= 3 || h1 == 12 || h1 <= 3) {
            is_straight_draw = true;
        }
    }

    let mut is_gutshot = false;
    if !is_straight_draw {
        for r in 0..9 {
            let count = (ranks[r] as u8)
                + (ranks[r + 1] as u8)
                + (ranks[r + 2] as u8)
                + (ranks[r + 3] as u8)
                + (ranks[r + 4] as u8);
            if count == 4 && ((h0 >= r && h0 <= r + 4) || (h1 >= r && h1 <= r + 4)) {
                is_gutshot = true;
                break;
            }
        }
        if !is_gutshot && ranks[12] {
            let count =
                1 + (ranks[0] as u8) + (ranks[1] as u8) + (ranks[2] as u8) + (ranks[3] as u8);
            if count == 4 && (h0 == 12 || h0 <= 3 || h1 == 12 || h1 <= 3) {
                is_gutshot = true;
            }
        }
    }

    DrawInfo {
        is_flush_draw,
        is_nut_flush_draw,
        is_straight_draw,
        is_gutshot,
    }
}

/* Backward-compatible draw detection returning (is_flush_draw, is_straight_draw) */
#[inline(always)]
pub fn detect_draws(hole: &[Card; 2], board: &[Card]) -> (bool, bool) {
    let d = detect_draws_detailed(hole, board);
    (d.is_flush_draw, d.is_straight_draw)
}
