/*
 * Card Abstraction and Bucketing Engine for 52-Card Texas Hold'em.
 *   1. Preflop: 169 canonical hand groups (Pairs, Suited, Offsuited).
 *   2. Postflop: Quantized score & Monte Carlo EHS equity bucketing.
 */

use crate::game::holdem::{evaluate_7cards, Card, Rank};

/* Canonical Preflop Hand Index (0..168) & Name ("AA", "AKs", "AKo", etc.) */
pub fn preflop_bucket(c1: Card, c2: Card) -> (usize, &'static str) {
    let (r1, r2) = if (c1.rank as u8) >= (c2.rank as u8) {
        (c1.rank, c2.rank)
    } else {
        (c2.rank, c1.rank)
    };

    let is_pair = r1 == r2;
    let is_suited = c1.suit == c2.suit;

    if is_pair {
        let idx = (Rank::Ace as usize) - (r1 as usize);
        let names = [
            "AA", "KK", "QQ", "JJ", "TT", "99", "88", "77", "66", "55", "44", "33", "22",
        ];
        (idx, names[idx])
    } else if is_suited {
        /* 78 Suited Hands: AKs, AQs ... 32s */
        let high = (Rank::Ace as usize) - (r1 as usize);
        let low = (Rank::Ace as usize) - (r2 as usize);
        /* Triangle index calculation */
        let offset = 13;
        let mut pair_count = 0;
        for h in 0..high {
            pair_count += 12 - h;
        }
        let idx = offset + pair_count + (low - high - 1);
        (idx, suited_name(r1, r2))
    } else {
        /* 78 Offsuited Hands: AKo, AQo ... 32o */
        let high = (Rank::Ace as usize) - (r1 as usize);
        let low = (Rank::Ace as usize) - (r2 as usize);
        let offset = 13 + 78;
        let mut pair_count = 0;
        for h in 0..high {
            pair_count += 12 - h;
        }
        let idx = offset + pair_count + (low - high - 1);
        (idx, offsuited_name(r1, r2))
    }
}

#[allow(dead_code)]
fn rank_char(r: Rank) -> &'static str {
    match r {
        Rank::Ace => "A",
        Rank::King => "K",
        Rank::Queen => "Q",
        Rank::Jack => "J",
        Rank::Ten => "T",
        Rank::Nine => "9",
        Rank::Eight => "8",
        Rank::Seven => "7",
        Rank::Six => "6",
        Rank::Five => "5",
        Rank::Four => "4",
        Rank::Three => "3",
        Rank::Two => "2",
    }
}

fn suited_name(r1: Rank, r2: Rank) -> &'static str {
    match (r1, r2) {
        (Rank::Ace, Rank::King) => "AKs",
        (Rank::Ace, Rank::Queen) => "AQs",
        (Rank::Ace, Rank::Jack) => "AJs",
        (Rank::Ace, Rank::Ten) => "ATs",
        (Rank::Ace, Rank::Nine) => "A9s",
        (Rank::Ace, Rank::Eight) => "A8s",
        (Rank::Ace, Rank::Seven) => "A7s",
        (Rank::Ace, Rank::Six) => "A6s",
        (Rank::Ace, Rank::Five) => "A5s",
        (Rank::Ace, Rank::Four) => "A4s",
        (Rank::Ace, Rank::Three) => "A3s",
        (Rank::Ace, Rank::Two) => "A2s",
        (Rank::King, Rank::Queen) => "KQs",
        (Rank::King, Rank::Jack) => "KJs",
        (Rank::King, Rank::Ten) => "KTs",
        (Rank::King, Rank::Nine) => "K9s",
        (Rank::King, Rank::Eight) => "K8s",
        (Rank::King, Rank::Seven) => "K7s",
        (Rank::King, Rank::Six) => "K6s",
        (Rank::King, Rank::Five) => "K5s",
        (Rank::King, Rank::Four) => "K4s",
        (Rank::King, Rank::Three) => "K3s",
        (Rank::King, Rank::Two) => "K2s",
        (Rank::Queen, Rank::Jack) => "QJs",
        (Rank::Queen, Rank::Ten) => "QTs",
        (Rank::Queen, Rank::Nine) => "Q9s",
        (Rank::Queen, Rank::Eight) => "Q8s",
        (Rank::Queen, Rank::Seven) => "Q7s",
        (Rank::Queen, Rank::Six) => "Q6s",
        (Rank::Queen, Rank::Five) => "Q5s",
        (Rank::Queen, Rank::Four) => "Q4s",
        (Rank::Queen, Rank::Three) => "Q3s",
        (Rank::Queen, Rank::Two) => "Q2s",
        (Rank::Jack, Rank::Ten) => "JTs",
        (Rank::Jack, Rank::Nine) => "J9s",
        (Rank::Jack, Rank::Eight) => "J8s",
        (Rank::Jack, Rank::Seven) => "J7s",
        (Rank::Jack, Rank::Six) => "J6s",
        (Rank::Jack, Rank::Five) => "J5s",
        (Rank::Jack, Rank::Four) => "J4s",
        (Rank::Jack, Rank::Three) => "J3s",
        (Rank::Jack, Rank::Two) => "J2s",
        (Rank::Ten, Rank::Nine) => "T9s",
        (Rank::Ten, Rank::Eight) => "T8s",
        (Rank::Ten, Rank::Seven) => "T7s",
        (Rank::Ten, Rank::Six) => "T6s",
        (Rank::Ten, Rank::Five) => "T5s",
        (Rank::Ten, Rank::Four) => "T4s",
        (Rank::Ten, Rank::Three) => "T3s",
        (Rank::Ten, Rank::Two) => "T2s",
        (Rank::Nine, Rank::Eight) => "98s",
        (Rank::Nine, Rank::Seven) => "97s",
        (Rank::Nine, Rank::Six) => "96s",
        (Rank::Nine, Rank::Five) => "95s",
        (Rank::Nine, Rank::Four) => "94s",
        (Rank::Nine, Rank::Three) => "93s",
        (Rank::Nine, Rank::Two) => "92s",
        (Rank::Eight, Rank::Seven) => "87s",
        (Rank::Eight, Rank::Six) => "86s",
        (Rank::Eight, Rank::Five) => "85s",
        (Rank::Eight, Rank::Four) => "84s",
        (Rank::Eight, Rank::Three) => "83s",
        (Rank::Eight, Rank::Two) => "82s",
        (Rank::Seven, Rank::Six) => "76s",
        (Rank::Seven, Rank::Five) => "75s",
        (Rank::Seven, Rank::Four) => "74s",
        (Rank::Seven, Rank::Three) => "73s",
        (Rank::Seven, Rank::Two) => "72s",
        (Rank::Six, Rank::Five) => "65s",
        (Rank::Six, Rank::Four) => "64s",
        (Rank::Six, Rank::Three) => "63s",
        (Rank::Six, Rank::Two) => "62s",
        (Rank::Five, Rank::Four) => "54s",
        (Rank::Five, Rank::Three) => "53s",
        (Rank::Five, Rank::Two) => "52s",
        (Rank::Four, Rank::Three) => "43s",
        (Rank::Four, Rank::Two) => "42s",
        (Rank::Three, Rank::Two) => "32s",
        _ => "XXs",
    }
}

fn offsuited_name(r1: Rank, r2: Rank) -> &'static str {
    match (r1, r2) {
        (Rank::Ace, Rank::King) => "AKo",
        (Rank::Ace, Rank::Queen) => "AQo",
        (Rank::Ace, Rank::Jack) => "AJo",
        (Rank::Ace, Rank::Ten) => "ATo",
        (Rank::Ace, Rank::Nine) => "A9o",
        (Rank::Ace, Rank::Eight) => "A8o",
        (Rank::Ace, Rank::Seven) => "A7o",
        (Rank::Ace, Rank::Six) => "A6o",
        (Rank::Ace, Rank::Five) => "A5o",
        (Rank::Ace, Rank::Four) => "A4o",
        (Rank::Ace, Rank::Three) => "A3o",
        (Rank::Ace, Rank::Two) => "A2o",
        (Rank::King, Rank::Queen) => "KQo",
        (Rank::King, Rank::Jack) => "KJo",
        (Rank::King, Rank::Ten) => "KTo",
        (Rank::King, Rank::Nine) => "K9o",
        (Rank::King, Rank::Eight) => "K8o",
        (Rank::King, Rank::Seven) => "K7o",
        (Rank::King, Rank::Six) => "K6o",
        (Rank::King, Rank::Five) => "K5o",
        (Rank::King, Rank::Four) => "K4o",
        (Rank::King, Rank::Three) => "K3o",
        (Rank::King, Rank::Two) => "K2o",
        (Rank::Queen, Rank::Jack) => "QJo",
        (Rank::Queen, Rank::Ten) => "QTo",
        (Rank::Queen, Rank::Nine) => "Q9o",
        (Rank::Queen, Rank::Eight) => "Q8o",
        (Rank::Queen, Rank::Seven) => "Q7o",
        (Rank::Queen, Rank::Six) => "Q6o",
        (Rank::Queen, Rank::Five) => "Q5o",
        (Rank::Queen, Rank::Four) => "Q4o",
        (Rank::Queen, Rank::Three) => "Q3o",
        (Rank::Queen, Rank::Two) => "Q2o",
        (Rank::Jack, Rank::Ten) => "JTo",
        (Rank::Jack, Rank::Nine) => "J9o",
        (Rank::Jack, Rank::Eight) => "J8o",
        (Rank::Jack, Rank::Seven) => "J7o",
        (Rank::Jack, Rank::Six) => "J6o",
        (Rank::Jack, Rank::Five) => "J5o",
        (Rank::Jack, Rank::Four) => "J4o",
        (Rank::Jack, Rank::Three) => "J3o",
        (Rank::Jack, Rank::Two) => "J2o",
        (Rank::Ten, Rank::Nine) => "T9o",
        (Rank::Ten, Rank::Eight) => "T8o",
        (Rank::Ten, Rank::Seven) => "T7o",
        (Rank::Ten, Rank::Six) => "T6o",
        (Rank::Ten, Rank::Five) => "T5o",
        (Rank::Ten, Rank::Four) => "T4o",
        (Rank::Ten, Rank::Three) => "T3o",
        (Rank::Ten, Rank::Two) => "T2o",
        (Rank::Nine, Rank::Eight) => "98o",
        (Rank::Nine, Rank::Seven) => "97o",
        (Rank::Nine, Rank::Six) => "96o",
        (Rank::Nine, Rank::Five) => "95o",
        (Rank::Nine, Rank::Four) => "94o",
        (Rank::Nine, Rank::Three) => "93o",
        (Rank::Nine, Rank::Two) => "92o",
        (Rank::Eight, Rank::Seven) => "87o",
        (Rank::Eight, Rank::Six) => "86o",
        (Rank::Eight, Rank::Five) => "85o",
        (Rank::Eight, Rank::Four) => "84o",
        (Rank::Eight, Rank::Three) => "83o",
        (Rank::Eight, Rank::Two) => "82o",
        (Rank::Seven, Rank::Six) => "76o",
        (Rank::Seven, Rank::Five) => "75o",
        (Rank::Seven, Rank::Four) => "74o",
        (Rank::Seven, Rank::Three) => "73o",
        (Rank::Seven, Rank::Two) => "72o",
        (Rank::Six, Rank::Five) => "65o",
        (Rank::Six, Rank::Four) => "64o",
        (Rank::Six, Rank::Three) => "63o",
        (Rank::Six, Rank::Two) => "62o",
        (Rank::Five, Rank::Four) => "54o",
        (Rank::Five, Rank::Three) => "53o",
        (Rank::Five, Rank::Two) => "52o",
        (Rank::Four, Rank::Three) => "43o",
        (Rank::Four, Rank::Two) => "42o",
        (Rank::Three, Rank::Two) => "32o",
        _ => "XXo",
    }
}

use crate::game::holdem::ALL_52_CARDS;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;

/* Computes Expected Hand Strength (EHS / Equity) via Monte Carlo rollouts or River enumeration. */
pub fn calculate_ehs_equity(hole: &[Card; 2], board: &[Card]) -> f64 {
    let mut is_used = [false; 52];
    let card_idx = |c: Card| -> usize { (c.suit as usize) * 13 + (c.rank as usize) };
    is_used[card_idx(hole[0])] = true;
    is_used[card_idx(hole[1])] = true;
    for &b in board {
        is_used[card_idx(b)] = true;
    }

    let remaining_deck: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|&c| !is_used[card_idx(c)])
        .collect();

    let num_board_missing = 5 - board.len();

    /* Fast deterministic evaluation on River */
    if num_board_missing == 0 {
        let my_score = evaluate_7cards(hole, board);
        let n = remaining_deck.len();
        let mut wins = 0.0f64;
        let mut total = 0.0f64;

        for i in 0..n {
            for j in (i + 1)..n {
                let opp_hole = [remaining_deck[i], remaining_deck[j]];
                let opp_score = evaluate_7cards(&opp_hole, board);
                if my_score > opp_score {
                    wins += 1.0;
                } else if my_score == opp_score {
                    wins += 0.5;
                }
                total += 1.0;
            }
        }
        return if total > 0.0 { wins / total } else { 0.5 };
    }

    /* Monte Carlo simulation for Flop (3 cards) and Turn (4 cards) */
    let num_trials = if num_board_missing == 1 { 120 } else { 80 };
    let mut wins = 0.0f64;
    let seed = ((card_idx(hole[0]) as u64) << 32)
        ^ ((card_idx(hole[1]) as u64) << 24)
        ^ (board.len() as u64);
    let mut rng = SmallRng::seed_from_u64(seed ^ 0xDEAD_BEEF_CAFE_BABE);

    let mut deck = remaining_deck;
    for _ in 0..num_trials {
        deck.shuffle(&mut rng);
        let opp_hole = [deck[0], deck[1]];
        let mut full_board = Vec::with_capacity(5);
        full_board.extend_from_slice(board);
        full_board.extend_from_slice(&deck[2..2 + num_board_missing]);

        let my_score = evaluate_7cards(hole, &full_board);
        let opp_score = evaluate_7cards(&opp_hole, &full_board);

        if my_score > opp_score {
            wins += 1.0;
        } else if my_score == opp_score {
            wins += 0.5;
        }
    }

    wins / (num_trials as f64)
}

/* Fast O(1) draw detection for Flop & Turn (Flush draws, Nut Flush draws, Straight draws, Gutshots) */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawInfo {
    pub is_flush_draw: bool,
    pub is_nut_flush_draw: bool,
    pub is_straight_draw: bool, // OESD (8 outs)
    pub is_gutshot: bool,       // Inside straight draw (4 outs)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardFlushTexture {
    Rainbow,
    TwoTone,
    Monotone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardTextureInfo {
    pub flush_texture: BoardFlushTexture,
    pub is_paired: bool,
    pub is_connected: bool,
}

pub fn detect_board_texture(board: &[Card]) -> BoardTextureInfo {
    if board.is_empty() {
        return BoardTextureInfo {
            flush_texture: BoardFlushTexture::Rainbow,
            is_paired: false,
            is_connected: false,
        };
    }
    let mut suit_counts = [0u8; 4];
    let mut rank_counts = [0u8; 13];
    for &c in board {
        suit_counts[c.suit as usize] += 1;
        rank_counts[c.rank as usize] += 1;
    }
    let max_suit = *suit_counts.iter().max().unwrap_or(&0);
    let flush_texture = if max_suit >= 3 {
        BoardFlushTexture::Monotone
    } else if max_suit == 2 {
        BoardFlushTexture::TwoTone
    } else {
        BoardFlushTexture::Rainbow
    };

    let is_paired = rank_counts.iter().any(|&cnt| cnt >= 2);

    let mut is_connected = false;
    let mut consecutive = 0;
    for &cnt in &rank_counts {
        if cnt > 0 {
            consecutive += 1;
            if consecutive >= 2 {
                is_connected = true;
                break;
            }
        } else {
            consecutive = 0;
        }
    }
    if !is_connected && rank_counts[12] > 0 && (rank_counts[0] > 0 || rank_counts[11] > 0) {
        is_connected = true;
    }

    BoardTextureInfo {
        flush_texture,
        is_paired,
        is_connected,
    }
}

/* Fast high-resolution draw-aware and texture-aware postflop equity bucketing (0..76).
 * Differentiates:
 * - Monster Combos (FD + OESD, FD + Gutshot)
 * - Nut Flush Draws vs Weak Flush Draws
 * - Open-Ended Straight Draws vs Gutshots
 * - Sets (hidden pocket pair) vs Trips (board paired)
 * - Top Pair Top Kicker vs Mid/Weak Kickers
 * - Dry vs Wet redraws
 */
pub fn postflop_equity_bucket(hole: &[Card; 2], board: &[Card]) -> usize {
    let score = evaluate_7cards(hole, board);
    let category = score >> 32;

    let bucket = match category {
        8 => 76, /* Straight Flush */
        7 => 75, /* Quads */
        6 => {
            /* Full House */
            let trips_rank = ((score >> 16) & 0xF) as usize;
            if trips_rank >= 10 {
                74 /* Aces full / Kings full / Queens full */
            } else if trips_rank >= 5 {
                73 /* Mid full house */
            } else {
                72 /* Low full house */
            }
        }
        5 => {
            /* Flush */
            let high_flush = ((score >> 16) & 0xF) as usize;
            match high_flush {
                12 => 71,     /* Nut Ace-high flush */
                11 => 70,     /* King-high flush */
                9..=10 => 69, /* Queen/Jack-high flush */
                6..=8 => 68,  /* Mid flush */
                _ => 67,      /* Low flush */
            }
        }
        4 => {
            /* Straight */
            let st_high = (score & 0xF) as usize;
            match st_high {
                12 => 66,      /* Broadway A-high straight */
                10..=11 => 65, /* King/Queen-high straight */
                7..=9 => 64,   /* Mid straight */
                4..=6 => 63,   /* Low straight */
                _ => 62,       /* 5-high Wheel (A-2-3-4-5) */
            }
        }
        3 => {
            /* Three of a Kind (Trips vs Set) */
            let trips_rank = ((score >> 16) & 0xF) as usize;
            let is_pocket_pair = hole[0].rank == hole[1].rank;
            if is_pocket_pair {
                /* Disguised Monster Set */
                if trips_rank >= 10 {
                    60 /* Top Set (AA, KK, QQ) */
                } else if trips_rank >= 6 {
                    59 /* Mid Set (JJ, TT, 99, 88) */
                } else {
                    58 /* Low Set */
                }
            } else {
                /* Visible Trips (board paired) */
                let kicker = if hole[0].rank as usize == trips_rank {
                    hole[1].rank as usize
                } else {
                    hole[0].rank as usize
                };
                if kicker >= 11 {
                    57 /* Trips Top Kicker (Ace, King) */
                } else if kicker >= 7 {
                    56 /* Trips Mid Kicker */
                } else {
                    55 /* Trips Weak Kicker */
                }
            }
        }
        2 => {
            /* Two Pair */
            let top_pair = ((score >> 16) & 0xF) as usize;
            let bottom_pair = (score & 0xF) as usize;
            let is_pocket_pair = hole[0].rank == hole[1].rank;
            let hit_top =
                (hole[0].rank as usize == top_pair) || (hole[1].rank as usize == top_pair);
            let hit_bottom =
                (hole[0].rank as usize == bottom_pair) || (hole[1].rank as usize == bottom_pair);

            if !is_pocket_pair && !hit_top && !hit_bottom {
                /* Both pairs are on board */
                let high_kicker = (hole[0].rank as usize).max(hole[1].rank as usize);
                if high_kicker >= 11 {
                    47 /* Board Two Pair with Ace/King kicker */
                } else {
                    46 /* Board Two Pair with low kicker */
                }
            } else if is_pocket_pair {
                if hole[0].rank as usize > top_pair {
                    54 /* Pocket pair above board pair */
                } else {
                    48 /* Pocket pair below board pair */
                }
            } else if hit_top && hit_bottom {
                let draws = detect_draws_detailed(hole, board);
                let base = if top_pair >= 10 {
                    52 /* High Two Pair (AK, AQ, AJ) */
                } else if top_pair >= 7 {
                    51 /* Mid Two Pair */
                } else {
                    49 /* Low Two Pair */
                };
                if draws.is_flush_draw {
                    53 /* Two Pair with Flush Redraw */
                } else {
                    base
                }
            } else {
                50 /* Top and Bottom Pair */
            }
        }
        1 => {
            /* One Pair (accurately distinguishing overpairs, top pairs, redraws, and board pairs) */
            let pair_rank = ((score >> 16) & 0xF) as usize;
            let is_pocket_pair = hole[0].rank == hole[1].rank;
            let hole_hit_pair =
                (hole[0].rank as usize == pair_rank) || (hole[1].rank as usize == pair_rank);

            if !is_pocket_pair && !hole_hit_pair {
                /* Pair is on board; player only has kicker */
                let high_kicker = (hole[0].rank as usize).max(hole[1].rank as usize);
                if high_kicker >= 11 {
                    29 /* Board pair with high kicker (Ace/King) */
                } else {
                    28 /* Board pair with low kicker */
                }
            } else {
                let max_board_rank = board.iter().map(|c| c.rank as usize).max().unwrap_or(0);
                let draws = detect_draws_detailed(hole, board);

                if is_pocket_pair && pair_rank > max_board_rank {
                    /* Overpair */
                    if draws.is_flush_draw || draws.is_straight_draw {
                        45 /* Overpair + Flush/Straight Draw */
                    } else if pair_rank >= 11 {
                        44 /* High Overpair: AA, KK */
                    } else if pair_rank >= 9 {
                        43 /* Mid Overpair: QQ, JJ */
                    } else {
                        42 /* Low Overpair: TT, 99, 88 */
                    }
                } else if hole_hit_pair && pair_rank >= max_board_rank {
                    /* Top Pair */
                    let kicker = if hole[0].rank as usize == pair_rank {
                        hole[1].rank as usize
                    } else {
                        hole[0].rank as usize
                    };
                    if draws.is_flush_draw {
                        41 /* Top Pair + Flush Draw */
                    } else if draws.is_straight_draw || draws.is_gutshot {
                        40 /* Top Pair + Straight/Gutshot Draw */
                    } else if kicker >= 11 {
                        39 /* Top Pair Top Kicker (TPTK: AK, AQ) */
                    } else if kicker >= 8 {
                        38 /* Top Pair Good Kicker */
                    } else if kicker >= 5 {
                        37 /* Top Pair Mid Kicker */
                    } else {
                        36 /* Top Pair Weak Kicker */
                    }
                } else if pair_rank > 6 {
                    /* Middle Pair */
                    if draws.is_flush_draw || draws.is_straight_draw {
                        34 /* Middle Pair + Draw */
                    } else if is_pocket_pair {
                        35 /* Pocket pair between board cards */
                    } else {
                        let kicker = (hole[0].rank as usize).max(hole[1].rank as usize);
                        if kicker >= 10 {
                            33 /* Middle Pair, high kicker */
                        } else {
                            32 /* Middle Pair, low kicker */
                        }
                    }
                } else {
                    /* Low / Bottom Pair */
                    if draws.is_flush_draw || draws.is_straight_draw {
                        31 /* Bottom Pair + Draw */
                    } else if is_pocket_pair {
                        30 /* Underpair */
                    } else {
                        28 /* Bottom Pair dry */
                    }
                }
            }
        }
        _ => {
            /* High Card or Draws on Flop/Turn vs River */
            let high_rank = ((score >> 16) & 0xF) as usize;
            if board.len() < 5 {
                let draws = detect_draws_detailed(hole, board);
                if draws.is_flush_draw && draws.is_straight_draw {
                    25 /* Flush Draw + OESD (15 outs combo draw) */
                } else if draws.is_flush_draw && draws.is_gutshot {
                    24 /* Flush Draw + Gutshot (12 outs combo draw) */
                } else if draws.is_nut_flush_draw {
                    if high_rank == 12 {
                        23 /* Nut Flush Draw with Ace */
                    } else {
                        22 /* Nut Flush Draw */
                    }
                } else if draws.is_flush_draw {
                    if high_rank >= 10 {
                        21 /* High Flush Draw (KQ high) */
                    } else if high_rank >= 7 {
                        20 /* Mid Flush Draw */
                    } else {
                        19 /* Low Flush Draw */
                    }
                } else if draws.is_straight_draw {
                    if high_rank >= 11 {
                        17 /* OESD with Two Overcards / Broadway */
                    } else if high_rank >= 9 {
                        16 /* OESD with One Overcard */
                    } else {
                        15 /* Low/Mid OESD */
                    }
                } else if draws.is_gutshot {
                    if high_rank >= 11 {
                        13 /* Gutshot with Two Overcards */
                    } else if high_rank >= 9 {
                        12 /* Gutshot with One Overcard */
                    } else {
                        11 /* Weak Gutshot */
                    }
                } else {
                    /* Pure High Card without draw on Flop/Turn */
                    match high_rank {
                        12 => {
                            let kicker = ((score >> 12) & 0xF) as usize;
                            if kicker >= 9 {
                                10
                            } else if kicker >= 5 {
                                9
                            } else {
                                8
                            }
                        }
                        11 => 7,
                        10 => 6,
                        9 => 5,
                        8 => 4,
                        7 => 3,
                        6 => 2,
                        5 => 1,
                        _ => 0,
                    }
                }
            } else {
                /* River (No draws possible, pure showdown high card value) */
                match high_rank {
                    12 => 10,
                    11 => 8,
                    10 => 6,
                    9 => 5,
                    8 => 4,
                    7 => 3,
                    6 => 2,
                    5 => 1,
                    _ => 0,
                }
            }
        }
    };

    bucket.min(76)
}

use crate::game::holdem::RoundHistory;

pub trait HistoryActions {
    fn actions_in_round(&self, r_idx: usize) -> &[u8];
}

impl HistoryActions for [RoundHistory; 4] {
    #[inline(always)]
    fn actions_in_round(&self, r_idx: usize) -> &[u8] {
        &self[r_idx]
    }
}

impl HistoryActions for [Vec<u8>; 4] {
    #[inline(always)]
    fn actions_in_round(&self, r_idx: usize) -> &[u8] {
        &self[r_idx]
    }
}

#[inline(always)]
pub fn action_to_char(a: u8) -> char {
    match a {
        0 => 'f',
        1 => 'c',
        2 => 'r',
        3 => 't',
        4 => 'h',
        5 => 'a',
        _ => 'x',
    }
}

/* Fast zero-allocation infoset key formatting into a reusable String buffer */
#[inline]
pub fn format_holdem_infoset_key_slice(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    round_history: &[u8],
    out: &mut String,
) {
    out.clear();
    if round == 1 {
        let (_, name) = preflop_bucket(hole[0], hole[1]);
        out.push_str("P:");
        out.push_str(name);
        out.push('/');
    } else {
        let bucket = postflop_equity_bucket(hole, board);
        let round_code = match round {
            2 => "F:B",
            3 => "T:B",
            4 => "R:B",
            _ => "X:B",
        };
        out.push_str(round_code);
        if bucket < 10 {
            out.push('0');
        }
        use std::fmt::Write;
        let _ = write!(out, "{}", bucket);
        out.push('/');
    }
    for &a in round_history {
        out.push(action_to_char(a));
    }
}

#[inline]
pub fn format_holdem_infoset_key<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
    out: &mut String,
) {
    let r_idx = (round.saturating_sub(1)) as usize;
    format_holdem_infoset_key_slice(hole, board, round, history.actions_in_round(r_idx), out);
}

/* Generates compact abstracted Information Set key for Texas Hold'em */
pub fn get_holdem_infoset_key<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
) -> String {
    let mut s = String::with_capacity(16);
    format_holdem_infoset_key(hole, board, round, history, &mut s);
    s
}

/* Rich Information Set key formatting that retains previous-street action context (e.g. preflop 3-bet vs limp) */
#[inline]
pub fn format_holdem_infoset_key_rich<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
    out: &mut String,
) {
    out.clear();
    if round == 1 {
        let (_, name) = preflop_bucket(hole[0], hole[1]);
        out.push_str("P:");
        out.push_str(name);
        out.push('/');
        for &a in history.actions_in_round(0) {
            out.push(action_to_char(a));
        }
    } else {
        let pf_actions = history.actions_in_round(0);
        if !pf_actions.is_empty() {
            out.push_str("P:");
            for &a in pf_actions {
                out.push(action_to_char(a));
            }
            out.push('|');
        }
        let bucket = postflop_equity_bucket(hole, board);
        let round_code = match round {
            2 => "F:B",
            3 => "T:B",
            4 => "R:B",
            _ => "X:B",
        };
        out.push_str(round_code);
        if bucket < 10 {
            out.push('0');
        }
        use std::fmt::Write;
        let _ = write!(out, "{}", bucket);
        out.push('/');
        let r_idx = (round.saturating_sub(1)) as usize;
        for &a in history.actions_in_round(r_idx) {
            out.push(action_to_char(a));
        }
    }
}

pub fn get_holdem_infoset_key_rich<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
) -> String {
    let mut s = String::with_capacity(24);
    format_holdem_infoset_key_rich(hole, board, round, history, &mut s);
    s
}

/* ── Fast 64-bit Hash Infoset Keys (Zero-allocation FNV-1a) ───────────────── */

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[inline(always)]
fn fnv1a_combine(mut hash: u64, byte: u8) -> u64 {
    hash ^= byte as u64;
    hash.wrapping_mul(FNV_PRIME)
}

#[inline]
pub fn hash_holdem_infoset_key_slice(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    round_history: &[u8],
) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    h = fnv1a_combine(h, round);
    h = fnv1a_combine(h, b':');
    if round == 1 {
        let (idx, _) = preflop_bucket(hole[0], hole[1]);
        h = fnv1a_combine(h, (idx & 0xff) as u8);
        h = fnv1a_combine(h, ((idx >> 8) & 0xff) as u8);
    } else {
        let bucket = postflop_equity_bucket(hole, board);
        h = fnv1a_combine(h, bucket as u8);
    }
    h = fnv1a_combine(h, b'/');
    for &a in round_history {
        h = fnv1a_combine(h, a);
    }
    h
}

#[inline]
pub fn hash_holdem_infoset_key<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
) -> u64 {
    let r_idx = (round.saturating_sub(1)) as usize;
    hash_holdem_infoset_key_slice(hole, board, round, history.actions_in_round(r_idx))
}

#[inline]
pub fn hash_holdem_infoset_key_rich<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    if round == 1 {
        h = fnv1a_combine(h, 1);
        h = fnv1a_combine(h, b':');
        let (idx, _) = preflop_bucket(hole[0], hole[1]);
        h = fnv1a_combine(h, (idx & 0xff) as u8);
        h = fnv1a_combine(h, ((idx >> 8) & 0xff) as u8);
        h = fnv1a_combine(h, b'/');
        for &a in history.actions_in_round(0) {
            h = fnv1a_combine(h, a);
        }
    } else {
        let pf_actions = history.actions_in_round(0);
        if !pf_actions.is_empty() {
            h = fnv1a_combine(h, b'P');
            for &a in pf_actions {
                h = fnv1a_combine(h, a);
            }
            h = fnv1a_combine(h, b'|');
        }
        h = fnv1a_combine(h, round);
        h = fnv1a_combine(h, b':');
        let bucket = postflop_equity_bucket(hole, board);
        h = fnv1a_combine(h, bucket as u8);
        h = fnv1a_combine(h, b'/');
        let r_idx = (round.saturating_sub(1)) as usize;
        for &a in history.actions_in_round(r_idx) {
            h = fnv1a_combine(h, a);
        }
    }
    h
}
