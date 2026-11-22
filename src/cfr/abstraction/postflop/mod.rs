/*!
 * Postflop 88-bucket (0..=87) equity mapping and category-specific evaluators.
 */
mod high_card;
mod pairs;

use crate::game::config::NUM_HOLE_CARDS;
use crate::game::holdem::{Card, evaluate_7cards};

use super::draws::detect_draws_detailed;
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
pub fn postflop_equity_bucket(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> usize {
    let score = evaluate_7cards(hole, board);
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

fn find_unseen_flush_rank(board: &[Card], flush_suit: usize, start_rank: usize) -> usize {
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
