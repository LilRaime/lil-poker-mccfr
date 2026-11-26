/*!
 * Postflop 88-bucket (0..=87) equity mapping and category-specific evaluators.
 */
mod extended;
mod high_card;
mod pairs;

use std::fmt;

use crate::game::config::NUM_HOLE_CARDS;
use crate::game::holdem::{Card, evaluate_7cards};

use super::draws::detect_draws_detailed;
pub use extended::*;
use high_card::bucket_high_card_or_draw;
use pairs::{bucket_one_pair, bucket_two_pair_or_paired_board};

pub const BUCKET_HIGH_CARD_MAX: usize = 10;
pub const BUCKET_WEAK_DRAW_MIN: usize = 11;
pub const BUCKET_STRONG_DRAW_MIN: usize = 15;
pub const BUCKET_WEAK_PAIR_MIN: usize = 28;
pub const BUCKET_TOP_PAIR_MIN: usize = 40;
pub const BUCKET_OVERPAIR_MIN: usize = 50;
pub const BUCKET_TWO_PAIR_MIN: usize = 57;
pub const BUCKET_TRIPS_MIN: usize = 65;
pub const BUCKET_STRAIGHT_MIN: usize = 73;
pub const BUCKET_STRAIGHT_MAX: usize = 78;
pub const BUCKET_FLUSH_MIN: usize = 79;
pub const BUCKET_STRONG_FLUSH_MIN: usize = 80;
pub const BUCKET_SECOND_NUT_FLUSH: usize = 82;
pub const BUCKET_NUT_FLUSH: usize = 83;
pub const BUCKET_FULL_HOUSE_MIN: usize = 84;
pub const BUCKET_MAX: usize = 87;

/** Zero-cost typed postflop abstraction bucket (`0..=87`) with semantic sub-bucket predicates */
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PostflopBucket(pub u8);

impl PostflopBucket {
    pub const PURE_RAG_AIR_MAX: u8 = 6;
    pub const STRONG_OVERCARDS_MIN: u8 = 7;
    pub const STRONG_ACE_KING_MIN: u8 = 9;
    pub const NON_NUT_DRAW_MAX: u8 = 21;
    pub const NUT_DRAW_MIN: u8 = 22;
    pub const COMBO_DRAW_MIN: u8 = 24;
    pub const NUT_COMBO_OR_PAIR_NFD_MIN: u8 = 26;
    pub const CRUSHED_PAIR: u8 = 28;
    pub const THREE_FLUSH_BOTTOM_PAIR: u8 = 29;
    pub const DRY_WEAK_BOTTOM_PAIR_MAX: u8 = 30;
    pub const BOTTOM_PAIR_REDRAW: u8 = 33;
    pub const CLEAN_MID_PAIR_MIN: u8 = 35;
    pub const DRY_MID_OR_UNDERPAIR_MAX: u8 = 37;
    pub const MID_PAIR_REDRAW: u8 = 38;
    pub const COUNTERFEITED_TOP_PAIR: u8 = 39;
    pub const FOUR_DANGER_TOP_PAIR: u8 = 40;
    pub const VULNERABLE_TOP_PAIR_MAX: u8 = 43;
    pub const DRY_WEAK_TOP_PAIR: u8 = 44;
    pub const STRONG_DRY_TOP_PAIR_MIN: u8 = 45;
    pub const HIGH_VULNERABLE_OVERPAIR: u8 = 52;
    pub const VULNERABLE_TWO_PAIR_MAX: u8 = 58;
    pub const CLEAN_TWO_PAIR_MIN: u8 = 61;
    pub const VULNERABLE_SET_MIN: u8 = 68;
    pub const TRIPS_OR_SET_FLUSH_REDRAW: u8 = 72;
    pub const BOARD_CHOP_OR_IDIOT_STRAIGHT: u8 = 73;
    pub const PAIRED_BOARD_STRAIGHT: u8 = 74;
    pub const CLEAN_UNPAIRED_STRAIGHT_MIN: u8 = 75;
    pub const GOOD_TWO_CARD_FLUSH: u8 = 81;

    #[inline(always)]
    pub const fn new(raw: u8) -> Self {
        Self(if raw > BUCKET_MAX as u8 {
            BUCKET_MAX as u8
        } else {
            raw
        })
    }

    #[inline(always)]
    pub const fn from_usize(raw: usize) -> Self {
        Self(if raw > BUCKET_MAX {
            BUCKET_MAX as u8
        } else {
            raw as u8
        })
    }

    #[inline(always)]
    pub const fn as_u8(self) -> u8 {
        self.0
    }

    #[inline(always)]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }

    #[inline(always)]
    pub const fn in_range(self, min: u8, max_inclusive: u8) -> bool {
        self.0 >= min && self.0 <= max_inclusive
    }

    #[inline(always)]
    pub const fn is_pure_rag_air(self) -> bool {
        self.0 <= Self::PURE_RAG_AIR_MAX
    }

    #[inline(always)]
    pub const fn is_high_card(self) -> bool {
        (self.0 as usize) < BUCKET_WEAK_DRAW_MIN
    }

    #[inline(always)]
    pub const fn is_air_or_weak_draw(self) -> bool {
        (self.0 as usize) < BUCKET_STRONG_DRAW_MIN
    }

    #[inline(always)]
    pub const fn is_unpaired(self) -> bool {
        (self.0 as usize) < BUCKET_WEAK_PAIR_MIN
    }

    #[inline(always)]
    pub const fn is_strong_draw(self) -> bool {
        self.0 >= BUCKET_STRONG_DRAW_MIN as u8 && self.0 < BUCKET_WEAK_PAIR_MIN as u8
    }

    #[inline(always)]
    pub const fn is_non_nut_draw(self) -> bool {
        self.0 >= BUCKET_STRONG_DRAW_MIN as u8 && self.0 <= Self::NON_NUT_DRAW_MAX
    }

    #[inline(always)]
    pub const fn is_nut_or_combo_draw(self) -> bool {
        self.0 >= Self::NUT_DRAW_MIN && self.0 < BUCKET_WEAK_PAIR_MIN as u8
    }

    #[inline(always)]
    pub const fn is_bare_nut_flush_draw(self) -> bool {
        self.0 == 22 || self.0 == 23
    }

    #[inline(always)]
    pub const fn is_combo_draw(self) -> bool {
        self.0 >= Self::COMBO_DRAW_MIN && self.0 <= 27
    }

    #[inline(always)]
    pub const fn is_nut_combo_or_pair_nfd(self) -> bool {
        self.0 == 26 || self.0 == 27
    }

    #[inline(always)]
    pub const fn is_multiway_nut_draw(self) -> bool {
        matches!(self.0, 22 | 23 | 25 | 27)
    }

    #[inline(always)]
    pub const fn is_crushed_pair(self) -> bool {
        self.0 == Self::CRUSHED_PAIR
    }

    #[inline(always)]
    pub const fn is_crushed_or_three_flush_bottom_pair(self) -> bool {
        self.0 == Self::CRUSHED_PAIR || self.0 == Self::THREE_FLUSH_BOTTOM_PAIR
    }

    #[inline(always)]
    pub const fn is_weak_bottom_pair(self) -> bool {
        self.0 >= BUCKET_WEAK_PAIR_MIN as u8 && self.0 <= Self::DRY_WEAK_BOTTOM_PAIR_MAX
    }

    #[inline(always)]
    pub const fn is_weak_or_mid_pair(self) -> bool {
        self.0 >= BUCKET_WEAK_PAIR_MIN as u8 && self.0 < BUCKET_TOP_PAIR_MIN as u8
    }

    #[inline(always)]
    pub const fn has_pair_redraw(self) -> bool {
        self.0 == Self::BOTTOM_PAIR_REDRAW || self.0 == Self::MID_PAIR_REDRAW
    }

    #[inline(always)]
    pub const fn is_bare_weak_or_mid_pair(self) -> bool {
        self.is_weak_or_mid_pair() && !self.has_pair_redraw()
    }

    #[inline(always)]
    pub const fn is_bare_weak_underpair(self) -> bool {
        self.0 >= BUCKET_WEAK_PAIR_MIN as u8 && self.0 <= 34 && self.0 != Self::BOTTOM_PAIR_REDRAW
    }

    #[inline(always)]
    pub const fn is_clean_mid_pair(self) -> bool {
        self.0 >= Self::CLEAN_MID_PAIR_MIN && self.0 <= Self::MID_PAIR_REDRAW
    }

    #[inline(always)]
    pub const fn is_counterfeited_top_pair(self) -> bool {
        self.0 == Self::COUNTERFEITED_TOP_PAIR
    }

    #[inline(always)]
    pub const fn is_top_pair(self) -> bool {
        self.0 >= BUCKET_TOP_PAIR_MIN as u8 && self.0 < BUCKET_OVERPAIR_MIN as u8
    }

    #[inline(always)]
    pub const fn is_strong_top_pair_or_overpair(self) -> bool {
        self.0 >= Self::STRONG_DRY_TOP_PAIR_MIN && self.0 < BUCKET_TWO_PAIR_MIN as u8
    }

    #[inline(always)]
    pub const fn is_thin_value_tp_or_two_pair(self) -> bool {
        self.0 >= Self::STRONG_DRY_TOP_PAIR_MIN && self.0 <= 63
    }

    #[inline(always)]
    pub const fn is_vulnerable_two_pair(self) -> bool {
        self.0 >= BUCKET_TWO_PAIR_MIN as u8 && self.0 <= Self::VULNERABLE_TWO_PAIR_MAX
    }

    #[inline(always)]
    pub const fn is_two_pair(self) -> bool {
        self.0 >= BUCKET_TWO_PAIR_MIN as u8 && self.0 < BUCKET_TRIPS_MIN as u8
    }

    #[inline(always)]
    pub const fn is_trips_or_set(self) -> bool {
        self.0 >= BUCKET_TRIPS_MIN as u8 && self.0 <= Self::TRIPS_OR_SET_FLUSH_REDRAW
    }

    #[inline(always)]
    pub const fn is_set_or_trips_redraw(self) -> bool {
        self.0 >= Self::VULNERABLE_SET_MIN && self.0 <= Self::TRIPS_OR_SET_FLUSH_REDRAW
    }

    #[inline(always)]
    pub const fn has_trips_flush_redraw(self) -> bool {
        self.0 == Self::TRIPS_OR_SET_FLUSH_REDRAW
    }

    #[inline(always)]
    pub const fn is_board_chop_or_idiot_straight(self) -> bool {
        self.0 == Self::BOARD_CHOP_OR_IDIOT_STRAIGHT
    }

    #[inline(always)]
    pub const fn is_paired_board_straight(self) -> bool {
        self.0 == Self::PAIRED_BOARD_STRAIGHT
    }

    #[inline(always)]
    pub const fn is_non_idiot_straight(self) -> bool {
        self.0 >= Self::PAIRED_BOARD_STRAIGHT && self.0 <= BUCKET_STRAIGHT_MAX as u8
    }
}

impl fmt::Display for PostflopBucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/** Typed wrapper over `postflop_equity_bucket` returning `PostflopBucket` */
#[inline]
pub fn postflop_bucket(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> PostflopBucket {
    PostflopBucket::from_usize(postflop_equity_bucket(hole, board))
}

/** Fast high-resolution draw-aware and texture-aware postflop equity bucketing (`0..=87`).
 * Differentiates:
 * - Unpaired High Card (0..=10), including board-pair / board-trips / board-2P kickers
 * - Weak Draws (11..=14) & Strong Draws (15..=27)
 * - Weak & Middle Pairs (28..=39), isolating counterfeited/4-flush/4-straight pairs at 28
 * - Texture-split Top Pair (40..=49: Wet/Paired 41..=43 vs Dry 44..=47 + Redraws 48..=49)
 * - Texture-split Overpairs (50..=56: Wet/Paired 51..=52 vs Dry 53..=55 + Redraw 56)
 * - Genuine Two Pair (57..=64, only when both hole cards hit distinct board ranks)
 * - Trips & Sets (65..=72)
 * - Straights (73..=78, isolating board/idiot-end straights at 73)
 * - Flushes (79..=83) & Full House / Quads / Straight Flush (84..=87)
 */
#[inline]
pub fn postflop_equity_bucket(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> usize {
    postflop_equity_bucket_with_score(hole, board, evaluate_7cards(hole, board))
}

#[inline]
pub fn postflop_equity_bucket_with_score(
    hole: &[Card; NUM_HOLE_CARDS],
    board: &[Card],
    score: u64,
) -> usize {
    let category = score >> 32;

    let bucket = match category {
        8 => bucket_straight_flush(score, board),
        7 => bucket_quads(score, *hole, board),
        6 => bucket_full_house(score, board),
        5 => bucket_flush(*hole, board),
        4 => bucket_straight(score, *hole, board),
        3 => bucket_trips_or_set(score, *hole, board),
        2 => bucket_two_pair_or_paired_board(score, *hole, board),
        1 => bucket_one_pair(score, *hole, board),
        _ => bucket_high_card_or_draw(*hole, board),
    };

    bucket.min(BUCKET_MAX)
}

fn bucket_straight_flush(score: u64, board: &[Card]) -> usize {
    if board.len() >= 5 {
        let b_score = evaluate_7cards(&[board[0], board[1]], &board[2..]);
        if (b_score >> 32) == 8 && b_score >= score {
            return 73; /* Playing the board Straight Flush (chop) */
        }
    }
    87
}

fn bucket_quads(score: u64, hole: [Card; NUM_HOLE_CARDS], board: &[Card]) -> usize {
    let quads_rank = ((score >> 16) & 0xF) as usize;
    let board_quads = board
        .iter()
        .filter(|c| c.rank as usize == quads_rank)
        .count()
        >= 4;
    if !board_quads {
        return 86;
    }
    if board.len() >= 5 {
        let b_score = evaluate_7cards(&[board[0], board[1]], &board[2..]);
        if (b_score >> 32) == 7 && b_score >= score {
            return 73; /* Board Quads chop */
        }
    }
    let hole_high = (hole[0].rank as usize).max(hole[1].rank as usize);
    if hole_high == 12 {
        86 /* Ace kicker plays unbeatable nut hand on board quads */
    } else if hole_high == 11 {
        78 /* King kicker */
    } else {
        hole_high.min(10)
    }
}

/** Checks if the board contains 4-to-a-straight for a higher straight than `st_high`,
 * and the player does not hold any higher straight card. */
fn is_idiot_end_straight(hole: [Card; NUM_HOLE_CARDS], board: &[Card], st_high: usize) -> bool {
    if board.len() < 4 || st_high >= 12 {
        return false;
    }
    let mut ranks_present = [false; 13];
    for c in board {
        ranks_present[c.rank as usize] = true;
    }
    let player_has = |rank: usize| hole[0].rank as usize == rank || hole[1].rank as usize == rank;

    for hr in (st_high + 1)..=12 {
        if hr >= 4 {
            let start = hr - 4;
            let board_cards = (start..=hr).filter(|&r| ranks_present[r]).count();
            if board_cards >= 4 && !player_has(hr) {
                return true;
            }
        }
    }
    false
}

fn bucket_full_house(score: u64, board: &[Card]) -> usize {
    let trips_rank = ((score >> 16) & 0xF) as usize;
    let pair_rank = (score & 0xF) as usize;
    let is_board_fh = if board.len() >= 5 {
        let b_score = evaluate_7cards(&[board[0], board[1]], &board[2..]);
        (b_score >> 32) == 6 && b_score >= score
    } else {
        false
    };
    let board_trips_count = board
        .iter()
        .filter(|c| c.rank as usize == trips_rank)
        .count();
    let max_other_board = board
        .iter()
        .map(|c| c.rank as usize)
        .filter(|&r| r != trips_rank)
        .max()
        .unwrap_or(0);

    if is_board_fh {
        73 /* Pure board Full House (playing the board chop) */
    } else if (board_trips_count >= 3 && pair_rank < max_other_board) || trips_rank < 5 {
        84 /* Vulnerable underfull on 3-of-a-kind board or low full house */
    } else {
        85 /* Mid/High full house */
    }
}

pub(super) fn find_unseen_flush_rank(
    board: &[Card],
    flush_suit: usize,
    start_rank: usize,
) -> usize {
    let mut rank = start_rank;
    while board
        .iter()
        .any(|c| c.suit as usize == flush_suit && c.rank as usize == rank)
    {
        if rank == 0 {
            break;
        }
        rank -= 1;
    }
    rank
}

fn bucket_flush(hole: [Card; NUM_HOLE_CARDS], board: &[Card]) -> usize {
    let mut board_suit_counts = [0u8; 4];
    for c in board {
        board_suit_counts[c.suit as usize] += 1;
    }
    let mut suit_counts = board_suit_counts;
    suit_counts[hole[0].suit as usize] += 1;
    suit_counts[hole[1].suit as usize] += 1;
    let flush_suit = suit_counts.iter().position(|&cnt| cnt >= 5).unwrap_or(0);
    let is_four_flush_board = board_suit_counts[flush_suit] >= 4;

    let my_high_flush = hole
        .iter()
        .filter(|c| c.suit as usize == flush_suit)
        .map(|c| c.rank as usize)
        .max();

    let Some(my_high_flush) = my_high_flush else {
        return 79; /* Purely playing the board flush (chop / bluff-catch only) */
    };

    let nut_rank = find_unseen_flush_rank(board, flush_suit, 12);
    let second_nut_rank = find_unseen_flush_rank(board, flush_suit, nut_rank.saturating_sub(1));

    if my_high_flush == nut_rank {
        83 /* 1st-Nut Flush */
    } else if my_high_flush == second_nut_rank {
        82 /* 2nd-Nut Flush */
    } else if is_four_flush_board {
        if my_high_flush >= 9 {
            80 /* Queen/Jack-high flush on 4-flush board */
        } else {
            79 /* Baby/Low flush on 4-flush board */
        }
    } else if my_high_flush >= 6 {
        81 /* Mid/High 2-card flush */
    } else {
        80 /* Low 2-card flush */
    }
}

fn bucket_straight(score: u64, hole: [Card; NUM_HOLE_CARDS], board: &[Card]) -> usize {
    let st_high = (score & 0xF) as usize;
    let is_board_straight = if board.len() >= 5 {
        let b_score = evaluate_7cards(&[board[0], board[1]], &board[2..]);
        (b_score >> 32) == 4 && ((b_score & 0xF) as usize == st_high)
    } else {
        false
    };

    let mut rank_counts = [0u8; 13];
    for c in board {
        rank_counts[c.rank as usize] += 1;
    }
    let board_pairs = rank_counts.iter().filter(|&&cnt| cnt == 2).count();
    let board_trips = rank_counts.iter().any(|&cnt| cnt >= 3);

    if board_trips || board_pairs >= 2 || super::texture::is_four_flush_board(board) {
        40 /* Crushed straight on double-paired/trips or 4-flush board */
    } else if is_board_straight || is_idiot_end_straight(hole, board, st_high) {
        73 /* Board Straight / Idiot-End Dummy Straight */
    } else if board_pairs == 1 {
        74 /* Discounted straight on paired board */
    } else {
        match st_high {
            10..=12 => 78, /* Broadway / King / Queen-high straight */
            7..=9 => 77,   /* Mid straight */
            4..=6 => 76,   /* Low straight */
            _ => 75,       /* 5-high Wheel (A-2-3-4-5) */
        }
    }
}

fn bucket_trips_or_set(score: u64, hole: [Card; NUM_HOLE_CARDS], board: &[Card]) -> usize {
    let trips_rank = ((score >> 16) & 0xF) as usize;
    let is_pocket_pair = hole[0].rank == hole[1].rank && hole[0].rank as usize == trips_rank;
    let hole_hit_trips =
        (hole[0].rank as usize == trips_rank) || (hole[1].rank as usize == trips_rank);
    let draws = detect_draws_detailed(&hole, board);
    let dangerous_board = board.len() >= 4 && super::texture::is_dangerous_board(board);
    let four_flush_board = super::texture::is_four_flush_board(board);

    if is_pocket_pair {
        if board.len() < 5 && draws.is_flush_draw {
            72 /* Monster Set + Flush Redraw */
        } else if four_flush_board {
            50 /* Set on 4-flush board without flush is a bluff-catcher */
        } else if dangerous_board {
            68 /* Vulnerable Set on 3-flush / 4-straight board */
        } else if trips_rank >= 6 {
            71 /* High/Top Set (88..AA) */
        } else if trips_rank >= 3 {
            70 /* Mid Set (55..77) */
        } else {
            69 /* Low Set (22..44) */
        }
    } else if hole_hit_trips {
        let kicker = if hole[0].rank as usize == trips_rank {
            hole[1].rank as usize
        } else {
            hole[0].rank as usize
        };
        if board.len() < 5 && draws.is_flush_draw {
            72 /* Trips + Flush Redraw */
        } else if four_flush_board {
            40 /* Trips on 4-flush board without flush */
        } else if dangerous_board {
            65 /* Vulnerable Trips on 3-flush / 4-straight board */
        } else if kicker >= 11 {
            67 /* Trips Top Kicker (Ace, King) */
        } else if kicker >= 7 {
            66 /* Trips Mid Kicker */
        } else {
            65 /* Trips Weak Kicker */
        }
    } else {
        let high_kicker = (hole[0].rank as usize).max(hole[1].rank as usize);
        let low_kicker = (hole[0].rank as usize).min(hole[1].rank as usize);
        let max_other_board = board
            .iter()
            .map(|c| c.rank as usize)
            .filter(|&r| r != trips_rank)
            .max()
            .unwrap_or(0);
        if high_kicker > max_other_board && high_kicker == 12 {
            if low_kicker >= 9 {
                10 /* Board trips with Ace-Broadway kicker */
            } else {
                9 /* Board trips with Ace-rag kicker */
            }
        } else if high_kicker > max_other_board && high_kicker == 11 {
            8 /* Board trips with King kicker */
        } else {
            high_kicker.min(7) /* Board trips with weak/counterfeited kicker (pure air) */
        }
    }
}
