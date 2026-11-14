/*
 * Expected Hand Strength (EHS / Equity) Rollouts via Monte Carlo & River Enumeration.
 */

use crate::game::config::{NUM_CARDS_DECK, NUM_COMMUNITY_CARDS, NUM_HOLE_CARDS, NUM_RANKS};
use crate::game::holdem::{evaluate_7cards, Card, ALL_52_CARDS};
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;

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

/* Computes Range-Aware & Multi-Way Expected Hand Strength (Equity in 0.0..=1.0)
 * against opponent ranges filtered by observed VPIP and aggression context. */
pub fn calculate_range_equity(
    hole: &[Card; NUM_HOLE_CARDS],
    board: &[Card],
    opp_vpip: f64,
    facing_heavy_bet: bool,
    active_opponents: usize,
) -> f64 {
    use super::preflop::{preflop_bucket, preflop_strength_tier};

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

    if remaining_deck.len() < 2 {
        return 0.5;
    }

    let eff_vpip = opp_vpip.clamp(0.15, 0.95);
    let max_tier = if facing_heavy_bet {
        if eff_vpip > 0.65 {
            5
        } else if eff_vpip > 0.40 {
            4
        } else {
            3
        }
    } else if eff_vpip > 0.65 {
        7
    } else if eff_vpip > 0.40 {
        6
    } else {
        5
    };

    let num_opps = active_opponents.clamp(1, 5);
    let num_board_missing = NUM_COMMUNITY_CARDS.saturating_sub(board.len());

    /* Fast exact weighted enumeration on the River when Heads-Up */
    if num_board_missing == 0 && num_opps == 1 {
        let my_score = evaluate_7cards(hole, board);
        let n = remaining_deck.len();
        let mut weighted_wins = 0.0f64;
        let mut total_weight = 0.0f64;
        let board_paired = {
            let mut rc = [0u8; 13];
            for c in board {
                rc[c.rank as usize] += 1;
            }
            rc.iter().any(|&cnt| cnt >= 2)
        };

        for i in 0..n {
            for j in (i + 1)..n {
                let opp_hole = [remaining_deck[i], remaining_deck[j]];
                let (pf_idx, _) = preflop_bucket(opp_hole[0], opp_hole[1]);
                let tier = preflop_strength_tier(pf_idx);
                let mut w = if tier <= max_tier {
                    1.0
                } else if tier == max_tier + 1 {
                    0.35
                } else {
                    0.08
                };
                let opp_score = evaluate_7cards(&opp_hole, board);
                if facing_heavy_bet {
                    let opp_hits_board = opp_hole[0].rank == opp_hole[1].rank
                        || board
                            .iter()
                            .any(|b| b.rank == opp_hole[0].rank || b.rank == opp_hole[1].rank);
                    let cat = if board_paired && !opp_hits_board {
                        0
                    } else {
                        opp_score >> 32
                    };
                    w *= if cat >= 3 {
                        3.5
                    } else if cat == 2 {
                        2.2
                    } else if cat == 1 {
                        0.65
                    } else {
                        0.12
                    };
                }
                if my_score > opp_score {
                    weighted_wins += w;
                } else if my_score == opp_score {
                    weighted_wins += 0.5 * w;
                }
                total_weight += w;
            }
        }
        return if total_weight > 1e-9 {
            weighted_wins / total_weight
        } else {
            0.5
        };
    }

    /* Seeded Monte Carlo rollout for Preflop / Flop / Turn or Multi-way River */
    let num_trials = if num_board_missing == 0 {
        100
    } else if num_board_missing == 1 {
        96
    } else {
        72
    };
    let mut seed = ((card_idx(hole[0]) as u64) << 32)
        ^ ((card_idx(hole[1]) as u64) << 24)
        ^ ((board.len() as u64) << 16)
        ^ (num_opps as u64);
    for &b in board {
        seed = seed.wrapping_mul(31).wrapping_add(card_idx(b) as u64);
    }
    let mut rng = SmallRng::seed_from_u64(seed ^ 0xA5A5_5A5A_F0F0_0F0F);
    let mut deck = remaining_deck;
    let mut weighted_wins = 0.0f64;
    let mut total_weight = 0.0f64;

    for _ in 0..num_trials {
        deck.shuffle(&mut rng);
        if deck.len() < num_opps * 2 + num_board_missing {
            break;
        }
        let mut full_board = Vec::with_capacity(NUM_COMMUNITY_CARDS);
        full_board.extend_from_slice(board);
        full_board.extend_from_slice(&deck[num_opps * 2..num_opps * 2 + num_board_missing]);

        let my_score = evaluate_7cards(hole, &full_board);
        let mut trial_weight = 1.0f64;
        let mut best_opp_score = 0u64;

        for opp_i in 0..num_opps {
            let opp_hole = [deck[opp_i * 2], deck[opp_i * 2 + 1]];
            let (pf_idx, _) = preflop_bucket(opp_hole[0], opp_hole[1]);
            let tier = preflop_strength_tier(pf_idx);
            let w = if tier <= max_tier {
                1.0
            } else if tier == max_tier + 1 {
                0.35
            } else {
                0.10
            };
            trial_weight *= w;
            let opp_score = evaluate_7cards(&opp_hole, &full_board);
            if opp_score > best_opp_score {
                best_opp_score = opp_score;
            }
        }

        if my_score > best_opp_score {
            weighted_wins += trial_weight;
        } else if my_score == best_opp_score {
            weighted_wins += 0.5 * trial_weight;
        }
        total_weight += trial_weight;
    }

    if total_weight > 1e-9 {
        weighted_wins / total_weight
    } else {
        0.5
    }
}
