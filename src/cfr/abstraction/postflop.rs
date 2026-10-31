/*
 * Postflop 88-bucket (0..=87) equity mapping and Expected Hand Strength (EHS) rollouts.
 */

use crate::game::config::{NUM_CARDS_DECK, NUM_COMMUNITY_CARDS, NUM_HOLE_CARDS, NUM_RANKS};
use crate::game::holdem::{evaluate_7cards, Card, ALL_52_CARDS};
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;

use super::draws::detect_draws_detailed;

/* Computes Expected Hand Strength (EHS / Equity) via Monte Carlo rollouts or River enumeration. */
pub fn calculate_ehs_equity(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> f64 {
    let mut is_used = [false; NUM_CARDS_DECK];
    let card_idx = |c: Card| -> usize { (c.suit as usize) * NUM_RANKS + (c.rank as usize) };
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

    let num_board_missing = NUM_COMMUNITY_CARDS - board.len();

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
        let mut full_board = Vec::with_capacity(NUM_COMMUNITY_CARDS);
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

/* Checks if the board contains 4-to-a-straight for a higher straight than st_high,
 * and the player does not hold any higher straight card. */
fn is_idiot_end_straight(hole: &[Card; NUM_HOLE_CARDS], board: &[Card], st_high: usize) -> bool {
    if board.len() < 4 || st_high >= 12 {
        return false;
    }
    let mut ranks_present = [false; 13];
    for c in board {
        ranks_present[c.rank as usize] = true;
    }
    let player_has = |rank: usize| hole[0].rank as usize == rank || hole[1].rank as usize == rank;

    /* Check each potential higher straight end from st_high + 1 to 12 */
    for hr in (st_high + 1)..=12 {
        if hr >= 4 {
            let start = hr - 4;
            /* Check how many of the 5 cards needed for the straight ending at hr are on board */
            let board_cards = (start..=hr).filter(|&r| ranks_present[r]).count();
            if board_cards >= 4 && !player_has(hr) {
                /* Board offers a higher straight that we do not have the high card for */
                return true;
            }
        }
    }
    false
}

/* Fast high-resolution draw-aware and texture-aware postflop equity bucketing (0..=87).
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
        8 => 87, /* Straight Flush */
        7 => 86, /* Quads */
        6 => {
            /* Full House */
            let trips_rank = ((score >> 16) & 0xF) as usize;
            let pair_rank = ((score >> 12) & 0xF) as usize;
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
        5 => {
            /* Kicker-aware Flush Abstraction (79..=83):
             * Distinguishes nut flush, 2nd nut flush, mid/high 2-card flush, 4-flush baby flush, and board flush. */
            let mut board_suit_counts = [0u8; 4];
            for c in board {
                board_suit_counts[c.suit as usize] += 1;
            }
            let mut suit_counts = board_suit_counts;
            suit_counts[hole[0].suit as usize] += 1;
            suit_counts[hole[1].suit as usize] += 1;
            let flush_suit = suit_counts.iter().position(|&cnt| cnt >= 5).unwrap_or(0);
            let is_four_flush_board = board_suit_counts[flush_suit] >= 4;

            let my_flush_cards: Vec<usize> = hole
                .iter()
                .filter(|c| c.suit as usize == flush_suit)
                .map(|c| c.rank as usize)
                .collect();

            if my_flush_cards.is_empty() {
                /* Purely playing the board flush (chop / bluff-catch only) */
                79 /* Board Flush */
            } else {
                let my_high_flush = *my_flush_cards.iter().max().unwrap();

                /* Find the Nut rank for this suit (highest rank not on board) */
                let mut nut_rank = 12;
                while board
                    .iter()
                    .any(|c| c.suit as usize == flush_suit && c.rank as usize == nut_rank)
                {
                    if nut_rank == 0 {
                        break;
                    }
                    nut_rank -= 1;
                }

                /* Find the 2nd Nut rank for this suit */
                let mut second_nut_rank = nut_rank.saturating_sub(1);
                while board
                    .iter()
                    .any(|c| c.suit as usize == flush_suit && c.rank as usize == second_nut_rank)
                {
                    if second_nut_rank == 0 {
                        break;
                    }
                    second_nut_rank -= 1;
                }

                if my_high_flush == nut_rank {
                    83 /* 1st-Nut Flush */
                } else if my_high_flush == second_nut_rank {
                    82 /* 2nd-Nut Flush */
                } else if is_four_flush_board {
                    /* On a 4-flush board, any single card below 2nd-nut is a vulnerable low flush (79..=80) */
                    if my_high_flush >= 9 {
                        80 /* Queen/Jack-high flush on 4-flush board */
                    } else {
                        79 /* Baby/Low flush on 4-flush board (e.g. 5d or 3c) */
                    }
                } else if my_high_flush >= 6 {
                    81 /* Mid/High 2-card flush */
                } else {
                    80 /* Low 2-card flush */
                }
            }
        }
        4 => {
            /* Kicker-aware Straight Abstraction (73..=78):
             * Isolates board/idiot-end straights at 73 and discounted paired-board straights at 74. */
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

            if board_trips || board_pairs >= 2 {
                /* On a double-paired or trips board (e.g. 4-3-2-4-2 in Match 2),
                 * full houses and quads crush any straight; straight is merely a bluff-catcher */
                52
            } else if super::texture::is_four_flush_board(board) {
                /* On a 4-flush board, straight without a flush card loses to any single flush card */
                40
            } else if is_board_straight || is_idiot_end_straight(hole, board, st_high) {
                /* Player does not hold a higher card than the board straight,
                 * or holds the dummy / idiot end on a 4-to-straight board */
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
        3 => {
            /* Three of a Kind (Trips vs Set vs Board Trips: 65..=72) */
            let trips_rank = ((score >> 16) & 0xF) as usize;
            let is_pocket_pair =
                hole[0].rank == hole[1].rank && hole[0].rank as usize == trips_rank;
            let hole_hit_trips =
                (hole[0].rank as usize == trips_rank) || (hole[1].rank as usize == trips_rank);
            let draws = detect_draws_detailed(hole, board);
            let dangerous_board = board.len() >= 4 && super::texture::is_dangerous_board(board);
            let four_flush_board = super::texture::is_four_flush_board(board);

            if is_pocket_pair {
                /* Disguised Monster Set */
                if board.len() < 5 && draws.is_flush_draw {
                    72 /* Monster Set + Flush Redraw */
                } else if four_flush_board {
                    50 /* Set on 4-flush board without flush is a bluff-catcher */
                } else if dangerous_board {
                    69 /* Vulnerable Set on 3-flush / 4-straight board */
                } else if trips_rank >= 6 {
                    71 /* Mid/Top Set (88..AA) */
                } else {
                    70 /* Low Set (22..77) */
                }
            } else if hole_hit_trips {
                /* Visible Trips (player holds one trips card) */
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
                    68 /* Trips Top Kicker (Ace, King) */
                } else if kicker >= 7 {
                    67 /* Trips Mid Kicker */
                } else {
                    66 /* Trips Weak Kicker */
                }
            } else {
                /* All three trips cards are on the board; player only has unpaired high card (0..=10) */
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
        2 => {
            /* Two Pair (Genuine Two Pair 57..=64 vs One Pair on Paired Board 28..=56) */
            let top_pair = ((score >> 16) & 0xF) as usize;
            let bottom_pair = ((score >> 8) & 0xF) as usize;
            let is_pocket_pair = hole[0].rank == hole[1].rank;
            let hit_top =
                (hole[0].rank as usize == top_pair) || (hole[1].rank as usize == top_pair);
            let hit_bottom =
                (hole[0].rank as usize == bottom_pair) || (hole[1].rank as usize == bottom_pair);

            if !is_pocket_pair && !hit_top && !hit_bottom {
                /* Both pairs are on board; player only has unpaired high card (0..=10) */
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
            } else if !is_pocket_pair && hit_top && hit_bottom {
                /* Genuine Two Pair: BOTH hole cards hit two distinct board ranks (57..=64) */
                let draws = detect_draws_detailed(hole, board);
                let overcards_count = board
                    .iter()
                    .filter(|c| (c.rank as usize) > top_pair)
                    .count();
                let is_four_fl = super::texture::is_four_flush_board(board);
                let is_four_st = super::texture::is_four_straight_board(board);
                let is_vulnerable = super::texture::is_three_flush_board(board)
                    || super::texture::is_dangerous_board(board);

                if board.len() < 5 && draws.is_flush_draw {
                    64 /* Genuine Two Pair + Flush Redraw */
                } else if is_four_fl || is_four_st {
                    57 /* Two Pair on 4-flush or 4-straight board without flush/straight */
                } else if overcards_count >= 2 {
                    58 /* Two Pair under 2+ overcards (e.g. T3 on A-K-T-3-x) */
                } else if overcards_count == 1 {
                    59 /* Two Pair under 1 overcard (e.g. J3 on J-9-3-Q-6) */
                } else if is_vulnerable {
                    60 /* Genuine Top Two Pair on vulnerable 3-flush / connected board */
                } else if top_pair >= 10 {
                    63 /* High Genuine Top Two Pair (AK, AQ, AJ, KQ) */
                } else if top_pair >= 7 {
                    62 /* Mid Genuine Top Two Pair */
                } else {
                    61 /* Low Genuine Top Two Pair */
                }
            } else {
                /* One pair is on the board and the player has a Pocket Pair or hit ONE board rank.
                 * Evaluate as a One-Pair hand on a paired board (28..=56) so middle/bottom pairs
                 * and overpairs under overcards are never misclassified as Two Pair! */
                let draws = detect_draws_detailed(hole, board);
                let is_four_fl = super::texture::is_four_flush_board(board);
                let is_four_st = super::texture::is_four_straight_board(board);
                let max_board_rank = board.iter().map(|c| c.rank as usize).max().unwrap_or(0);

                let (my_pair_rank, kicker) = if is_pocket_pair {
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
                };

                let mut higher_rank_mask = 0u16;
                for c in board {
                    let r = c.rank as usize;
                    if r > my_pair_rank {
                        higher_rank_mask |= 1 << r;
                    }
                }
                let distinct_overcards = higher_rank_mask.count_ones() as usize;

                if is_pocket_pair && my_pair_rank > max_board_rank {
                    /* Overpair on a paired board (50..=56) */
                    if board.len() < 5 && (draws.is_flush_draw || draws.is_straight_draw) {
                        56 /* Overpair + Flush/Straight Redraw */
                    } else if is_four_fl || is_four_st {
                        50 /* Overpair on 4-flush or 4-straight board */
                    } else if my_pair_rank >= 11 {
                        52 /* Vulnerable High Overpair (AA, KK) on paired board */
                    } else {
                        51 /* Vulnerable Low/Mid Overpair (e.g. TT on 9-9-7) on paired board */
                    }
                } else if !is_pocket_pair && my_pair_rank >= max_board_rank {
                    /* Top Pair on a paired board (39..=49) */
                    let max_unpaired_board = board
                        .iter()
                        .map(|c| c.rank as usize)
                        .filter(|&r| r != top_pair && r != bottom_pair)
                        .max()
                        .unwrap_or(0);
                    let kicker_counterfeited = board.len() >= 5 && kicker <= max_unpaired_board;

                    if board.len() < 5 && draws.is_flush_draw {
                        49 /* Top Pair + Flush Redraw */
                    } else if board.len() < 5 && (draws.is_straight_draw || draws.is_gutshot) {
                        48 /* Top Pair + Straight/Gutshot Redraw */
                    } else if kicker_counterfeited {
                        39 /* Top Pair with counterfeited kicker on paired river (e.g. K5 on T-K-6-Q-6) */
                    } else if is_four_fl || is_four_st {
                        40 /* Top Pair on 4-flush or 4-straight paired board */
                    } else if kicker >= 11 {
                        43 /* Vulnerable Top Pair Top Kicker on paired board (e.g. AK on A-7-2-T-2) */
                    } else if kicker >= 6 {
                        42 /* Vulnerable Top Pair Mid/Good Kicker on paired board */
                    } else {
                        41 /* Vulnerable Top Pair Weak Kicker on paired board (e.g. A3 on T-8-A-8) */
                    }
                } else if is_pocket_pair {
                    /* Pocket Pair below at least 1 board rank on a paired board (28..=38) */
                    if board.len() < 5 && draws.is_nut_flush_draw {
                        26 /* Pocket Pair + Nut Flush Draw */
                    } else if board.len() < 5 && (draws.is_flush_draw || draws.is_straight_draw) {
                        38 /* Pocket Pair + Flush/Straight Draw */
                    } else if is_four_fl || is_four_st {
                        28 /* Pocket Pair on 4-flush / 4-straight paired board */
                    } else if distinct_overcards >= 3 {
                        32 /* Pocket Pair under 3+ overcards */
                    } else if distinct_overcards == 2 {
                        34 /* Pocket Pair under 2 overcards (e.g. TT on 9-9-7-Q-K) */
                    } else {
                        37 /* Pocket Pair under 1 overcard on paired board (e.g. 88 on 9-9-4) */
                    }
                } else {
                    /* 2nd/Middle or 3rd/Bottom Pair on a paired board (28..=38) */
                    if board.len() < 5 && draws.is_nut_flush_draw {
                        26 /* Pair + Nut Flush Draw */
                    } else if board.len() < 5 && (draws.is_flush_draw || draws.is_straight_draw) {
                        if distinct_overcards == 1 {
                            38 /* Middle Pair + Draw */
                        } else {
                            33 /* Bottom Pair + Draw */
                        }
                    } else if is_four_fl || is_four_st {
                        28 /* Weak Pair on 4-flush / 4-straight paired board */
                    } else if distinct_overcards == 1 {
                        34 /* Middle/2nd Pair on paired board (e.g. 97 on 7-2-J-6-6) */
                    } else {
                        29 /* Bottom/3rd+ Pair on paired board (e.g. K8 on 9-4-8-A-9) */
                    }
                }
            }
        }
        1 => {
            /* One Pair (unpaired board: accurately distinguishing overpairs, top pairs, redraws, and board pairs) */
            let pair_rank = ((score >> 16) & 0xF) as usize;
            let is_pocket_pair = hole[0].rank == hole[1].rank;
            let hole_hit_pair =
                (hole[0].rank as usize == pair_rank) || (hole[1].rank as usize == pair_rank);
            let draws = detect_draws_detailed(hole, board);

            if !is_pocket_pair && !hole_hit_pair {
                /* Pair is on board (flop/turn); player only has unpaired high-card kicker or draw */
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
                    if high_kicker >= 11 {
                        13
                    } else {
                        12
                    }
                } else if high_kicker == 12 && low_kicker >= 9 {
                    10 /* Board pair with strong Ace-Broadway kicker (AK, AQ, AJ, AT) */
                } else if high_kicker == 12 {
                    9 /* Board pair with Ace-rag kicker (A2..A9) */
                } else if high_kicker == 11 {
                    8 /* Board pair with King kicker */
                } else {
                    high_kicker.min(7) /* Board pair with Queen-high or worse (pure air) */
                }
            } else {
                let max_board_rank = board.iter().map(|c| c.rank as usize).max().unwrap_or(0);
                let min_board_rank = board.iter().map(|c| c.rank as usize).min().unwrap_or(0);
                let is_four_fl = super::texture::is_four_flush_board(board);
                let is_four_st = super::texture::is_four_straight_board(board);
                let is_three_fl = super::texture::is_three_flush_board(board);
                let is_vulnerable_texture = is_three_fl
                    || super::texture::is_dangerous_board(board)
                    || super::texture::detect_board_texture(board).is_paired;

                if is_pocket_pair && pair_rank > max_board_rank {
                    /* Overpair (50..=56) */
                    if board.len() < 5 && (draws.is_flush_draw || draws.is_straight_draw) {
                        56 /* Overpair + Flush/Straight Draw */
                    } else if is_four_fl || is_four_st {
                        50 /* Overpair on 4-flush or 4-straight board without redraw */
                    } else if is_vulnerable_texture {
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
                } else if hole_hit_pair && pair_rank >= max_board_rank {
                    /* Top Pair (39..=49) */
                    let kicker = if hole[0].rank as usize == pair_rank {
                        hole[1].rank as usize
                    } else {
                        hole[0].rank as usize
                    };
                    /* Check if kicker is completely counterfeited by board (4 board cards higher than kicker) */
                    let kicker_plays =
                        board.iter().filter(|c| (c.rank as usize) > kicker).count() < 4;

                    if board.len() < 5 && draws.is_flush_draw {
                        49 /* Top Pair + Flush Draw */
                    } else if board.len() < 5 && (draws.is_straight_draw || draws.is_gutshot) {
                        48 /* Top Pair + Straight/Gutshot Draw */
                    } else if !kicker_plays {
                        39 /* Top Pair with completely counterfeited kicker (e.g. Q3 on T-9-5-4-Q) */
                    } else if is_four_fl || is_four_st {
                        40 /* Top Pair on 4-flush or 4-straight board */
                    } else if is_vulnerable_texture {
                        if kicker >= 11 {
                            43 /* Vulnerable Top Pair Top Kicker */
                        } else if kicker >= 6 {
                            42 /* Vulnerable Top Pair Mid/Good Kicker */
                        } else {
                            41 /* Vulnerable Top Pair Weak Kicker */
                        }
                    } else if kicker >= 11 {
                        47 /* Dry Top Pair Top Kicker (TPTK: AK, AQ) */
                    } else if kicker >= 8 {
                        46 /* Dry Top Pair Good Kicker */
                    } else if kicker >= 5 {
                        45 /* Dry Top Pair Mid Kicker */
                    } else {
                        44 /* Dry Top Pair Weak Kicker */
                    }
                } else if pair_rank > min_board_rank {
                    /* Middle Pair (relative to board ranks: 28..=38) */
                    let overcards_count = board
                        .iter()
                        .filter(|c| (c.rank as usize) > pair_rank)
                        .count();
                    if board.len() < 5 && draws.is_nut_flush_draw {
                        26 /* Middle/Bottom Pair + Nut Flush Draw */
                    } else if board.len() < 5 && (draws.is_flush_draw || draws.is_straight_draw) {
                        38 /* Middle Pair + Draw */
                    } else if is_four_fl || is_four_st {
                        28 /* Counterfeited / crushed pair on 4-straight or 4-flush board */
                    } else if is_pocket_pair {
                        if overcards_count >= 3 {
                            32 /* Pocket pair under 3+ board overcards (e.g. 88 on T-J-A-4-5) */
                        } else if overcards_count == 2 {
                            34 /* Pocket pair under 2 board overcards (e.g. 88 on T-J-4-5) */
                        } else {
                            37 /* Pocket pair under single overcard (e.g. 88 on T-4-5) */
                        }
                    } else {
                        let kicker = if hole[0].rank as usize == pair_rank {
                            hole[1].rank as usize
                        } else {
                            hole[0].rank as usize
                        };
                        if overcards_count >= 3 && is_three_fl {
                            28 /* Crushed 4th pair on 3-flush board */
                        } else if overcards_count >= 3 {
                            29 /* 4th pair on board is weak bottom pair */
                        } else if is_three_fl {
                            34 /* Middle Pair on 3-flush board */
                        } else if kicker >= 10 && overcards_count == 1 {
                            36 /* Dry Middle Pair, high kicker */
                        } else {
                            35 /* Dry Middle Pair, low/mid kicker */
                        }
                    }
                } else {
                    /* Low / Bottom Pair or Underpair (28..=33) */
                    if board.len() < 5 && draws.is_nut_flush_draw {
                        26 /* Bottom Pair + Nut Flush Draw */
                    } else if board.len() < 5 && (draws.is_flush_draw || draws.is_straight_draw) {
                        33 /* Bottom Pair + Draw */
                    } else if is_four_st || is_four_fl {
                        28 /* Bottom Pair / Underpair on 4-straight / 4-flush board */
                    } else if is_pocket_pair {
                        32 /* Underpair */
                    } else if is_three_fl {
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
            }
        }
        _ => {
            /* High Card or Draws on Flop/Turn vs River (0..=27) */
            let hole_high = (hole[0].rank as usize).max(hole[1].rank as usize);
            let hole_low = (hole[0].rank as usize).min(hole[1].rank as usize);
            let max_board_rank = board.iter().map(|c| c.rank as usize).max().unwrap_or(0);
            let overcards = (hole[0].rank as usize > max_board_rank) as usize
                + (hole[1].rank as usize > max_board_rank) as usize;
            let has_backdoor_fd = board.len() == 3
                && hole[0].suit == hole[1].suit
                && board.iter().any(|c| c.suit == hole[0].suit);

            if board.len() < 5 {
                let draws = detect_draws_detailed(hole, board);
                if draws.is_nut_flush_draw && draws.is_straight_draw {
                    27 /* Nut Flush Draw + OESD (monster 15 nut outs combo draw) */
                } else if draws.is_nut_flush_draw && draws.is_gutshot {
                    26 /* Nut Flush Draw + Gutshot (12 nut outs combo draw) */
                } else if draws.is_flush_draw && draws.is_straight_draw {
                    25 /* Flush Draw + OESD (15 outs combo draw) */
                } else if draws.is_flush_draw && draws.is_gutshot {
                    24 /* Flush Draw + Gutshot (12 outs combo draw) */
                } else if draws.is_nut_flush_draw {
                    if hole_high == 12 {
                        23 /* Nut Flush Draw with Ace */
                    } else {
                        22 /* Nut Flush Draw */
                    }
                } else if draws.is_flush_draw {
                    if hole_high >= 10 {
                        21 /* High Flush Draw (KQ high) */
                    } else if hole_high >= 7 {
                        20 /* Mid Flush Draw */
                    } else {
                        19 /* Low Flush Draw */
                    }
                } else if draws.is_straight_draw {
                    if has_backdoor_fd && overcards >= 1 {
                        18 /* OESD + Backdoor Flush Draw + Overcard */
                    } else if overcards == 2 || hole_high >= 11 {
                        17 /* OESD with Two Overcards / Broadway */
                    } else if overcards == 1 || hole_high >= 9 {
                        16 /* OESD with One Overcard */
                    } else {
                        15 /* Low/Mid OESD */
                    }
                } else if draws.is_gutshot {
                    if overcards == 2 || hole_high >= 11 {
                        13 /* Gutshot with Two Overcards */
                    } else if overcards == 1 || hole_high >= 9 {
                        12 /* Gutshot with One Overcard */
                    } else {
                        11 /* Weak Gutshot */
                    }
                } else if overcards == 2 && has_backdoor_fd && hole_high >= 11 {
                    14 /* Two Broadway Overcards + Backdoor Flush Draw */
                } else {
                    /* Pure High Card without draw on Flop/Turn */
                    match hole_high {
                        12 => {
                            if hole_low >= 9 {
                                10
                            } else {
                                9
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
                /* River (No draws possible, pure showdown high card value from hole cards) */
                match hole_high {
                    12 => {
                        if hole_low >= 9 {
                            10
                        } else {
                            9
                        }
                    }
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

    bucket.min(87)
}
