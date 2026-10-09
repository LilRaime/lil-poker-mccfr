/*
 * Postflop 77-bucket equity mapping and Expected Hand Strength (EHS) rollouts.
 */

use crate::game::holdem::{evaluate_7cards, Card, ALL_52_CARDS};
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;

use super::draws::detect_draws_detailed;

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
