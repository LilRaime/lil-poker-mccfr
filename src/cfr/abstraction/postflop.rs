/*
 * Postflop 77-bucket equity mapping and Expected Hand Strength (EHS) rollouts.
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

/* Fast high-resolution draw-aware and texture-aware postflop equity bucketing (0..76).
 * Differentiates:
 * - Monster Combos (FD + OESD, FD + Gutshot)
 * - Nut Flush Draws vs Weak Flush Draws
 * - Open-Ended Straight Draws vs Gutshots
 * - Sets (hidden pocket pair) vs Trips (board paired)
 * - Top Pair Top Kicker vs Mid/Weak Kickers
 * - Dry vs Wet redraws
 */
pub fn postflop_equity_bucket(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> usize {
    let score = evaluate_7cards(hole, board);
    let category = score >> 32;

    let bucket = match category {
        8 => 76, /* Straight Flush */
        7 => 75, /* Quads */
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
                62 /* Pure board Full House (playing the board chop) */
            } else if board_trips_count >= 3 && pair_rank < max_other_board {
                72 /* Vulnerable underfull on 3-of-a-kind board */
            } else if trips_rank >= 10 {
                74 /* Aces full / Kings full / Queens full */
            } else if trips_rank >= 5 {
                73 /* Mid full house */
            } else {
                72 /* Low full house */
            }
        }
        5 => {
            /* Kicker-aware Flush Abstraction:
             * Distinguishes nut flush, 2nd nut flush, mid/low flush, 4-flush baby flush, and board flush.
             * Prevents holding 4s 5s on A-J-T-3-2 monotone board or 3c on 8c-Kc-Tc-4h-Jc from being misclassified as Monster. */
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
                67 /* Board Flush */
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
                    71 /* Nut Flush */
                } else if my_high_flush == second_nut_rank {
                    70 /* 2nd Nut Flush */
                } else if is_four_flush_board {
                    /* On a 4-flush board, any single card below 2nd-nut is a vulnerable low flush (bucket 67..68) */
                    if my_high_flush >= 9 {
                        68 /* Queen/Jack-high flush on 4-flush board */
                    } else {
                        67 /* Baby/Low flush on 4-flush board (e.g. 5d or 3c) */
                    }
                } else if my_high_flush >= 9 {
                    69 /* Queen/Jack-high flush */
                } else if my_high_flush >= 6 {
                    68 /* Mid flush */
                } else {
                    67 /* Low flush */
                }
            }
        }
        4 => {
            /* Kicker-aware Straight Abstraction:
             * Accurately detects when player holds higher straight card vs playing the board straight,
             * and detects idiot-end straights on 4-to-straight boards.
             * Also discounts straights on paired / double-paired boards where full houses dominate (Match 2). */
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
                44
            } else if super::texture::is_four_flush_board(board) {
                /* On a 4-flush board, straight without a flush card loses to any single flush card */
                40
            } else if is_board_straight || is_idiot_end_straight(hole, board, st_high) {
                /* Player does not hold a higher card than the board straight,
                 * or holds the dummy / idiot end on a 4-to-straight board */
                62 /* Board Straight / Dummy Straight */
            } else {
                let base: usize = match st_high {
                    12 => 66,      /* Broadway A-high straight */
                    10..=11 => 65, /* King/Queen-high straight */
                    7..=9 => 64,   /* Mid straight */
                    4..=6 => 63,   /* Low straight */
                    _ => 62,       /* 5-high Wheel (A-2-3-4-5) */
                };
                if board_pairs == 1 {
                    base.saturating_sub(3) /* Discounted straight on paired board */
                } else {
                    base
                }
            }
        }
        3 => {
            /* Three of a Kind (Trips vs Set vs Board Trips) */
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
                    61 /* Monster Set + Flush Redraw */
                } else {
                    let base = if trips_rank >= 10 {
                        60 /* Top Set (AA, KK, QQ) */
                    } else if trips_rank >= 6 {
                        59 /* Mid Set (JJ, TT, 99, 88) */
                    } else {
                        58 /* Low Set */
                    };
                    if four_flush_board {
                        43 /* Set on 4-flush board without flush is a bluff-catcher */
                    } else if dangerous_board {
                        base - 3 /* Vulnerable Set on 3-flush / 4-straight board */
                    } else {
                        base
                    }
                }
            } else if hole_hit_trips {
                /* Visible Trips (player holds one trips card) */
                let kicker = if hole[0].rank as usize == trips_rank {
                    hole[1].rank as usize
                } else {
                    hole[0].rank as usize
                };
                if board.len() < 5 && draws.is_flush_draw {
                    61 /* Trips + Flush Redraw */
                } else {
                    let base = if kicker >= 11 {
                        57 /* Trips Top Kicker (Ace, King) */
                    } else if kicker >= 7 {
                        56 /* Trips Mid Kicker */
                    } else {
                        55 /* Trips Weak Kicker */
                    };
                    if four_flush_board {
                        41 /* Trips on 4-flush board without flush */
                    } else if dangerous_board {
                        base - 3 /* Vulnerable Trips on 3-flush / 4-straight board (52..54) */
                    } else {
                        base
                    }
                }
            } else {
                /* All three trips cards are on the board; player only has a kicker */
                let high_kicker = (hole[0].rank as usize).max(hole[1].rank as usize);
                let max_other_board = board
                    .iter()
                    .map(|c| c.rank as usize)
                    .filter(|&r| r != trips_rank)
                    .max()
                    .unwrap_or(0);
                if high_kicker > max_other_board && high_kicker == 12 {
                    29 /* Board trips with Ace kicker */
                } else if high_kicker > max_other_board && high_kicker == 11 {
                    28 /* Board trips with King kicker */
                } else {
                    high_kicker.min(9) /* Board trips with weak/counterfeited kicker (pure air) */
                }
            }
        }
        2 => {
            /* Two Pair */
            let top_pair = ((score >> 16) & 0xF) as usize;
            let bottom_pair = ((score >> 8) & 0xF) as usize;
            let is_pocket_pair = hole[0].rank == hole[1].rank;
            let hit_top =
                (hole[0].rank as usize == top_pair) || (hole[1].rank as usize == top_pair);
            let hit_bottom =
                (hole[0].rank as usize == bottom_pair) || (hole[1].rank as usize == bottom_pair);

            if !is_pocket_pair && !hit_top && !hit_bottom {
                /* Both pairs are on board; player only plays kicker */
                let high_kicker = (hole[0].rank as usize).max(hole[1].rank as usize);
                let max_unpaired_board = board
                    .iter()
                    .map(|c| c.rank as usize)
                    .filter(|&r| r != top_pair && r != bottom_pair)
                    .max()
                    .unwrap_or(0);
                if high_kicker > max_unpaired_board && high_kicker >= 11 {
                    29 /* Board Two Pair with Ace/King kicker playing over board */
                } else if high_kicker > max_unpaired_board && high_kicker == 10 {
                    28 /* Board Two Pair with Queen kicker */
                } else {
                    high_kicker.min(8) /* Counterfeited/weak kicker on double-paired board */
                }
            } else if is_pocket_pair {
                if super::texture::is_four_flush_board(board) {
                    38 /* Pocket pair on 4-flush board without flush is only a bluff-catcher */
                } else if super::texture::is_four_straight_board(board) {
                    42 /* Pocket pair on 4-straight board without straight is heavily discounted */
                } else if hole[0].rank as usize >= top_pair {
                    54 /* Pocket pair above board pair */
                } else {
                    48 /* Pocket pair below board pair */
                }
            } else if hit_top && hit_bottom {
                let draws = detect_draws_detailed(hole, board);
                let overcards_count = board
                    .iter()
                    .filter(|c| (c.rank as usize) > top_pair)
                    .count();
                let base = if overcards_count >= 2 {
                    43 /* Two Pair under 2+ overcards (e.g. T3 on A-K-T-3-x) */
                } else if overcards_count == 1 {
                    45 /* Two Pair under 1 overcard (e.g. T3 on Q-T-3-4-6, 2nd & 4th pair, Match 8) */
                } else if top_pair >= 10 {
                    52 /* High Genuine Top Two Pair (AK, AQ, AJ) */
                } else if top_pair >= 7 {
                    51 /* Mid Genuine Top Two Pair */
                } else {
                    49 /* Low Genuine Top Two Pair */
                };

                if draws.is_flush_draw {
                    if overcards_count == 0 {
                        53 /* Genuine Top Two Pair with Flush Redraw */
                    } else {
                        base + 2 /* Two pair with flush redraw */
                    }
                } else if super::texture::is_four_flush_board(board) {
                    38 /* Two Pair on 4-flush board without flush */
                } else if super::texture::is_four_straight_board(board) {
                    42.min(base) /* Two pair on 4-straight board without straight */
                } else {
                    base
                }
            } else {
                /* Board has a pair, player has ONE pair with the board */
                let (hit_rank, kicker) = if hit_top {
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

                if super::texture::is_four_flush_board(board) {
                    38 /* Paired board with 4-flush without flush is a bluff catcher */
                } else if super::texture::is_four_straight_board(board) {
                    40 /* Paired board with 4-straight without straight */
                } else if hit_rank == top_pair {
                    /* Player holds top pair on a paired board */
                    /* Check if hole kicker actually plays over board cards */
                    let max_unpaired_board = board
                        .iter()
                        .map(|c| c.rank as usize)
                        .filter(|&r| r != top_pair && r != bottom_pair)
                        .max()
                        .unwrap_or(0);

                    if kicker > max_unpaired_board && kicker >= 11 {
                        50 /* Top Pair on paired board with Ace/King kicker */
                    } else if kicker > max_unpaired_board && kicker >= 8 {
                        48 /* Top Pair on paired board with decent kicker */
                    } else {
                        47 /* Counterfeited kicker on paired board (kicker does not play) */
                    }
                } else {
                    /* Player holds bottom pair on a paired board */
                    46 /* Bottom pair on paired board (weak two pair) */
                }
            }
        }
        1 => {
            /* One Pair (accurately distinguishing overpairs, top pairs, redraws, and board pairs) */
            let pair_rank = ((score >> 16) & 0xF) as usize;
            let is_pocket_pair = hole[0].rank == hole[1].rank;
            let hole_hit_pair =
                (hole[0].rank as usize == pair_rank) || (hole[1].rank as usize == pair_rank);
            let draws = detect_draws_detailed(hole, board);

            if !is_pocket_pair && !hole_hit_pair {
                /* Pair is on board; player only has high-card kicker or draw */
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
                } else if high_kicker == 12 && low_kicker >= 9 {
                    29 /* Board pair with strong Ace-Broadway kicker (AK, AQ, AJ) */
                } else if high_kicker >= 11 {
                    28 /* Board pair with Ace-rag or King kicker (weak bluff-catcher) */
                } else {
                    high_kicker.min(9) /* Board pair with Queen-high or worse (pure air) */
                }
            } else {
                let max_board_rank = board.iter().map(|c| c.rank as usize).max().unwrap_or(0);
                let min_board_rank = board.iter().map(|c| c.rank as usize).min().unwrap_or(0);
                let dangerous_turn_river =
                    board.len() >= 4 && super::texture::is_dangerous_board(board);

                if is_pocket_pair && pair_rank > max_board_rank {
                    /* Overpair */
                    if draws.is_flush_draw || draws.is_straight_draw {
                        45 /* Overpair + Flush/Straight Draw */
                    } else {
                        let base = if pair_rank >= 11 {
                            44 /* High Overpair: AA, KK */
                        } else if pair_rank >= 9 {
                            43 /* Mid Overpair: QQ, JJ */
                        } else {
                            42 /* Low Overpair: TT, 99, 88 */
                        };
                        if dangerous_turn_river {
                            base - 2 /* Vulnerable Overpair on 4-flush / 4-straight board */
                        } else {
                            base
                        }
                    }
                } else if hole_hit_pair && pair_rank >= max_board_rank {
                    /* Top Pair */
                    let kicker = if hole[0].rank as usize == pair_rank {
                        hole[1].rank as usize
                    } else {
                        hole[0].rank as usize
                    };
                    /* Check if kicker is completely counterfeited by board (4 board cards higher than kicker) */
                    let kicker_plays =
                        board.iter().filter(|c| (c.rank as usize) > kicker).count() < 4;

                    if draws.is_flush_draw {
                        41 /* Top Pair + Flush Draw */
                    } else if draws.is_straight_draw || draws.is_gutshot {
                        40 /* Top Pair + Straight/Gutshot Draw */
                    } else if !kicker_plays {
                        35 /* Top Pair with completely counterfeited kicker (e.g. Q3 on T-9-5-4-Q, Match 1) */
                    } else {
                        let base = if kicker >= 11 {
                            39 /* Top Pair Top Kicker (TPTK: AK, AQ) */
                        } else if kicker >= 8 {
                            38 /* Top Pair Good Kicker */
                        } else if kicker >= 5 {
                            37 /* Top Pair Mid Kicker */
                        } else {
                            36 /* Top Pair Weak Kicker */
                        };
                        if dangerous_turn_river {
                            base - 2 /* Vulnerable Top Pair on 4-flush / 4-straight board */
                        } else {
                            base
                        }
                    }
                } else if pair_rank > min_board_rank {
                    /* Middle Pair (relative to board ranks) */
                    let overcards_count = board
                        .iter()
                        .filter(|c| (c.rank as usize) > pair_rank)
                        .count();
                    if draws.is_nut_flush_draw {
                        26 /* Middle/Bottom Pair + Nut Flush Draw */
                    } else if draws.is_flush_draw || draws.is_straight_draw {
                        34 /* Middle Pair + Draw */
                    } else if is_pocket_pair {
                        if overcards_count >= 3 {
                            30 /* Pocket pair under 3+ board overcards (e.g. 88 on T-J-A-4-5) */
                        } else if overcards_count == 2 {
                            32 /* Pocket pair under 2 board overcards (e.g. 88 on T-J-4-5) */
                        } else {
                            35 /* Pocket pair under single overcard (e.g. 88 on T-4-5) */
                        }
                    } else {
                        let kicker = if hole[0].rank as usize == pair_rank {
                            hole[1].rank as usize
                        } else {
                            hole[0].rank as usize
                        };
                        let is_four_st = super::texture::is_four_straight_board(board);
                        let is_four_fl = super::texture::is_four_flush_board(board);
                        let is_three_fl = super::texture::is_three_flush_board(board);

                        if (overcards_count >= 2 && (is_four_st || is_four_fl))
                            || (overcards_count >= 3 && is_three_fl)
                        {
                            20 /* Counterfeited / crushed pair on 4-straight or flush board (Match 5) */
                        } else if overcards_count >= 3 {
                            28 /* 4th pair on board is weak bottom pair */
                        } else if kicker >= 10 && overcards_count == 1 {
                            33 /* Middle Pair, high kicker */
                        } else {
                            32 /* Middle Pair, low kicker or multiple overcards */
                        }
                    }
                } else {
                    /* Low / Bottom Pair or Underpair */
                    let is_four_st = super::texture::is_four_straight_board(board);
                    let is_four_fl = super::texture::is_four_flush_board(board);

                    if draws.is_nut_flush_draw {
                        26 /* Bottom Pair + Nut Flush Draw */
                    } else if draws.is_flush_draw || draws.is_straight_draw {
                        31 /* Bottom Pair + Draw */
                    } else if is_pocket_pair {
                        30 /* Underpair */
                    } else if is_four_st || is_four_fl {
                        20 /* Bottom Pair on 4-straight / 4-flush board is pure air */
                    } else {
                        28 /* Bottom Pair dry */
                    }
                }
            }
        }
        _ => {
            /* High Card or Draws on Flop/Turn vs River (using player's own hole card ranks) */
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
                            } else if hole_low >= 5 {
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
                /* River (No draws possible, pure showdown high card value from hole cards) */
                match hole_high {
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
