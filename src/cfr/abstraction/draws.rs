/*
 * Fast O(1) draw detection for Flop & Turn (Flush draws, Nut Flush draws, Straight draws, Gutshots).
 */

use crate::game::config::{NUM_COMMUNITY_CARDS, NUM_HOLE_CARDS, NUM_RANKS, NUM_SUITS};
use crate::game::holdem::{Card, Rank};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawInfo {
    pub is_flush_draw: bool,
    pub is_nut_flush_draw: bool,
    pub is_straight_draw: bool,
    pub is_gutshot: bool,
}

#[inline(always)]
pub fn detect_draws_detailed(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> DrawInfo {
    if board.len() >= NUM_COMMUNITY_CARDS {
        return DrawInfo {
            is_flush_draw: false,
            is_nut_flush_draw: false,
            is_straight_draw: false,
            is_gutshot: false,
        };
    }

    let mut board_suit_counts = [0u8; NUM_SUITS];
    for &b in board {
        board_suit_counts[b.suit as usize] += 1;
    }

    let mut is_flush_draw = false;
    let mut is_nut_flush_draw = false;
    for (s, &b_count) in board_suit_counts.iter().enumerate() {
        let hole_has = (hole[0].suit as usize == s) as u8 + (hole[1].suit as usize == s) as u8;
        if hole_has >= 1 && (b_count + hole_has == 4) {
            let max_hole_suit_rank = hole
                .iter()
                .filter(|c| c.suit as usize == s)
                .map(|c| c.rank as usize)
                .max()
                .unwrap_or(0);
            let ace_on_board = board
                .iter()
                .any(|c| c.suit as usize == s && c.rank == Rank::Ace);
            let is_nut_card = max_hole_suit_rank == (Rank::Ace as usize)
                || (ace_on_board && max_hole_suit_rank == (Rank::King as usize));

            /* On a 3-flush board with only 1 suited hole card, only count Ace/King-high
             * redraws as real flush draws to avoid treating weak 1-card draws as monsters. */
            if hole_has == 2 || max_hole_suit_rank >= (Rank::King as usize) {
                is_flush_draw = true;
                if is_nut_card {
                    is_nut_flush_draw = true;
                }
            }
            break;
        }
    }

    let mut board_ranks = [false; NUM_RANKS];
    let mut ranks = [false; NUM_RANKS];
    for &b in board {
        board_ranks[b.rank as usize] = true;
        ranks[b.rank as usize] = true;
    }
    ranks[hole[0].rank as usize] = true;
    ranks[hole[1].rank as usize] = true;

    let h0 = hole[0].rank as usize;
    let h1 = hole[1].rank as usize;
    let mut is_straight_draw = false;
    for r in 0..9 {
        if ranks[r]
            && ranks[r + 1]
            && ranks[r + 2]
            && ranks[r + 3]
            && ((h0 >= r && h0 <= r + 3 && !board_ranks[h0])
                || (h1 >= r && h1 <= r + 3 && !board_ranks[h1]))
        {
            is_straight_draw = true;
            break;
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
            if count == 4
                && ((h0 >= r && h0 <= r + 4 && !board_ranks[h0])
                    || (h1 >= r && h1 <= r + 4 && !board_ranks[h1]))
            {
                is_gutshot = true;
                break;
            }
        }
        if !is_gutshot && ranks[12] {
            let count =
                1 + (ranks[0] as u8) + (ranks[1] as u8) + (ranks[2] as u8) + (ranks[3] as u8);
            if count == 4
                && (((h0 == 12 || h0 <= 3) && !board_ranks[h0])
                    || ((h1 == 12 || h1 <= 3) && !board_ranks[h1]))
            {
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
pub fn detect_draws(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> (bool, bool) {
    let d = detect_draws_detailed(hole, board);
    (d.is_flush_draw, d.is_straight_draw)
}

/* Fast blocker detection for River bluffing:
 * Checks whether the player holds a key blocker that removes the opponent's nut hands:
 * 1. Nut Flush Blocker: board has >= 3 cards of suit S, and player holds the Ace of suit S.
 * 2. Broadway Straight Blocker: board has >= 3 broadway cards, and player holds an Ace or King. */
#[inline]
pub fn detect_river_nut_blocker(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> bool {
    if board.len() < 5 {
        return false;
    }
    let mut board_suit_counts = [0u8; NUM_SUITS];
    for &b in board {
        board_suit_counts[b.suit as usize] += 1;
    }
    for (s, &cnt) in board_suit_counts.iter().enumerate() {
        if cnt >= 3
            && ((hole[0].rank == Rank::Ace && hole[0].suit as usize == s)
                || (hole[1].rank == Rank::Ace && hole[1].suit as usize == s))
        {
            return true;
        }
    }
    let broadway_count = board.iter().filter(|c| c.rank as usize >= 8).count();
    let ace_on_board = board.iter().any(|c| c.rank == Rank::Ace);
    if broadway_count >= 3
        && !ace_on_board
        && (hole[0].rank == Rank::Ace || hole[1].rank == Rank::Ace)
    {
        return true;
    }
    false
}
