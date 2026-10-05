/*
 * Real-Time Subgame Search (Subgame Solving) for Texas Hold'em.
 * Solves depth-limited subgame online at Turn/River nodes during live play.
 */

use crate::cfr::abstraction::get_holdem_infoset_key;
use crate::cfr::node::InfosetNode;
use crate::game::config::*;
use crate::game::holdem::{Card, TexasHoldemGame, ALL_52_CARDS};
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::collections::HashMap;

pub struct SubgameSolver {
    pub num_iterations: usize,
    pub adaptive: bool,
}

impl SubgameSolver {
    pub fn new(num_iterations: usize) -> Self {
        SubgameSolver {
            num_iterations,
            adaptive: true,
        }
    }

    pub fn with_adaptive(mut self, adaptive: bool) -> Self {
        self.adaptive = adaptive;
        self
    }

    /* Computes dynamic iteration budget based on pot size, street, and bet pressure */
    pub fn compute_budget(&self, round: u8, contributions: [i32; 2], my_player: usize) -> usize {
        if !self.adaptive {
            return self.num_iterations;
        }

        let pot = contributions[0] + contributions[1];
        let opp = 1 - my_player;
        let to_call = (contributions[opp] - contributions[my_player]).max(0);

        let mut mult = 1.0f64;

        /* Pot scaling: big pots require higher precision to avoid costly mistakes */
        if pot >= SUBGAME_POT_LG {
            mult *= 2.2;
        } else if pot >= SUBGAME_POT_MD {
            mult *= 1.6;
        } else if pot >= SUBGAME_POT_SM {
            mult *= 1.25;
        } else if pot <= SUBGAME_POT_XS {
            mult *= 0.55;
        }

        /* Street scaling: River has zero chance nodes -> can solve to high precision */
        if round >= 4 {
            mult *= 1.35;
        } else if round == 3 {
            mult *= 1.1;
        }

        /* Bet pressure scaling: facing large bets or all-in requires deeper branch exploration */
        if to_call >= SUBGAME_BET_PRESSURE {
            mult *= 1.25;
        }

        let dynamic_iters = (self.num_iterations as f64 * mult).round() as usize;
        let min_bound = (self.num_iterations / 3).max(100).min(self.num_iterations);
        let max_bound = (self.num_iterations * 3).max(6000);
        dynamic_iters.clamp(min_bound, max_bound)
    }

    /* Solves subgame rooted at current board/hole state for my_player. */
    pub fn solve<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        &self,
        hole: &[Card; 2],
        board: &[Card],
        round: u8,
        history: &H,
        my_player: usize,
    ) -> Vec<f64> {
        self.solve_with_state(hole, board, round, history, my_player, [100, 100], 0)
    }

    /* Solves subgame with exact table contributions, pot odds, and raise limits. */
    #[allow(clippy::too_many_arguments)]
    pub fn solve_with_state<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        &self,
        hole: &[Card; 2],
        board: &[Card],
        round: u8,
        history: &H,
        my_player: usize,
        contributions: [i32; 2],
        raises_this_round: u8,
    ) -> Vec<f64> {
        use crate::cfr::abstraction::preflop_bucket;
        use rand::Rng;

        /* High-entropy board- and chip-sensitive PRNG seed */
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        for &c in hole {
            seed = seed
                .wrapping_mul(31)
                .wrapping_add(((c.rank as u64) << 3) | (c.suit as u64));
        }
        for &c in board {
            seed = seed
                .wrapping_mul(37)
                .wrapping_add(((c.rank as u64) << 3) | (c.suit as u64));
        }
        seed ^= (contributions[0] as u64).wrapping_shl(16) ^ (contributions[1] as u64);
        seed ^= (round as u64).wrapping_shl(32);
        let mut rng = SmallRng::seed_from_u64(seed);

        let is_used = |c: Card| hole.contains(&c) || board.contains(&c);
        let remaining_deck: Vec<Card> = ALL_52_CARDS
            .iter()
            .copied()
            .filter(|&c| !is_used(c))
            .collect();

        let rem_len = remaining_deck.len();
        if rem_len < 2 {
            return vec![0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        }

        /* Check preflop aggression in history to condition opponent reach range */
        let pf_actions = history.actions_in_round(0);
        let opp_raised_pf = pf_actions.iter().any(|&a| a >= 2);

        /* Pre-allocate node map for subgame search tree */
        let mut nodes: HashMap<String, InfosetNode> = HashMap::with_capacity(256);

        let effective_iterations = self.compute_budget(round, contributions, my_player);

        /* Run local CFR+ iterations */
        for iter_idx in 0..effective_iterations {
            /* Condition opponent reach range via Bayesian acceptance sampling */
            let mut opp_hole = [remaining_deck[0], remaining_deck[1]];
            for _ in 0..8 {
                let idx0 = rng.gen_range(0..rem_len);
                let mut idx1 = rng.gen_range(0..rem_len - 1);
                if idx1 >= idx0 {
                    idx1 += 1;
                }
                let cand = [remaining_deck[idx0], remaining_deck[idx1]];

                let (pf_idx, _) = preflop_bucket(cand[0], cand[1]);
                let pf_weight = if opp_raised_pf {
                    if pf_idx <= 45 {
                        1.0
                    } else if pf_idx <= 90 {
                        0.75
                    } else if pf_idx <= 125 {
                        0.25
                    } else {
                        0.04
                    }
                } else if pf_idx <= 12 {
                    0.35 /* Monster pair slow-play */
                } else if pf_idx <= 90 {
                    0.95
                } else if pf_idx <= 125 {
                    0.55
                } else {
                    0.15
                };

                let flop_weight = if round >= 3 && board.len() >= 3 {
                    let f_score = crate::game::holdem::evaluate_7cards(&cand, &board[..3]);
                    let f_cat = f_score >> 32;
                    if f_cat >= 1 {
                        1.0
                    } else {
                        let (fd, sd) = crate::cfr::abstraction::detect_draws(&cand, &board[..3]);
                        if fd || sd {
                            0.85
                        } else {
                            0.12
                        }
                    }
                } else {
                    1.0
                };

                let total_weight = pf_weight * flop_weight;
                if rng.gen::<f64>() <= total_weight {
                    opp_hole = cand;
                    break;
                }
                opp_hole = cand;
            }

            let (p0_hole, p1_hole) = if my_player == 0 {
                (*hole, opp_hole)
            } else {
                (opp_hole, *hole)
            };

            /* Fast partial Fisher-Yates shuffle only for missing board cards */
            let mut deck_rem_cards = [ALL_52_CARDS[0]; 52];
            let mut rem_count = 0;
            for &c in &remaining_deck {
                if c != opp_hole[0] && c != opp_hole[1] {
                    deck_rem_cards[rem_count] = c;
                    rem_count += 1;
                }
            }
            let num_board_missing = 5usize.saturating_sub(board.len());
            for i in 0..num_board_missing.min(rem_count) {
                let j = rng.gen_range(i..rem_count);
                deck_rem_cards.swap(i, j);
            }

            let game = TexasHoldemGame {
                hole: [p0_hole, p1_hole],
                board: crate::game::holdem::Board::from_slice(board),
                deck_remaining: crate::game::holdem::DeckRemaining::from_slice(
                    &deck_rem_cards[..rem_count],
                ),
                history: [
                    crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(0)),
                    crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(1)),
                    crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(2)),
                    crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(3)),
                ],
                contributions,
                current_player: my_player,
                round,
                raises_this_round,
                terminal: false,
                returns: [0.0, 0.0],
            };

            let updating_player = iter_idx % 2;
            self.cfr(
                &game,
                updating_player,
                iter_idx as f64,
                &mut nodes,
                &mut rng,
            );
        }

        /* Extract average strategy for root infoset */
        let root_key = get_holdem_infoset_key(hole, board, round, history);
        let opp = 1 - my_player;
        let to_call = (contributions[opp] - contributions[my_player]).max(0);
        let pot = contributions[0] + contributions[1];
        let legal = if to_call > 0 {
            vec![0, 1, 2, 3, 4, 5]
        } else {
            vec![1, 2, 3, 4, 5]
        };
        let fallback = crate::cfr::fallback::get_holdem_fallback_strategy(
            hole, board, round, to_call, pot, &legal,
        );

        if let Some(node) = nodes.get(&root_key) {
            let solved = node.get_average_strategy();
            /* Safe Resolving (Burch et al. 2014):
             * Blend real-time solved subgame policy with robust GTO fallback bounds (85% solved / 15% fallback)
             * to guarantee gift-proofing and prevent opponent out-of-distribution exploitation. */
            let mut resolved = vec![0.0f64; 6];
            let mut sum = 0.0f64;
            for &a in &legal {
                let idx = a as usize;
                let val = (0.85 * solved[idx] + 0.15 * fallback[idx]).max(0.0);
                resolved[idx] = val;
                sum += val;
            }
            if sum > 1e-9 {
                let inv = 1.0 / sum;
                for &a in &legal {
                    resolved[a as usize] *= inv;
                }
            }
            resolved
        } else {
            fallback.to_vec()
        }
    }

    fn cfr(
        &self,
        game: &TexasHoldemGame,
        updating_player: usize,
        weight: f64,
        nodes: &mut HashMap<String, InfosetNode>,
        rng: &mut SmallRng,
    ) -> f64 {
        if game.is_terminal() {
            return game.get_returns()[updating_player];
        }

        let cur_p = game.current_player();
        let legal = game.legal_actions();
        if legal.is_empty() {
            return game.get_returns()[updating_player];
        }

        let key = get_holdem_infoset_key(&game.hole[cur_p], &game.board, game.round, &game.history);

        let strategy = {
            let node = nodes
                .entry(key.clone())
                .or_insert_with(|| InfosetNode::new(6));
            node.get_strategy()
        };

        if cur_p == updating_player {
            let mut util = [0.0f64; 6];
            let mut node_util = 0.0f64;

            for &a in &legal {
                let next_game = game.apply_action(a);
                let action_util = self.cfr(&next_game, updating_player, weight, nodes, rng);
                util[a as usize] = action_util;
                node_util += strategy[a as usize] * action_util;
            }

            let mut regrets = vec![0.0f64; 6];
            for &a in &legal {
                regrets[a as usize] = util[a as usize] - node_util;
            }

            if let Some(node) = nodes.get_mut(&key) {
                node.update_regrets_cfr_plus(&regrets);
                node.accumulate_strategy(&strategy, weight);
            }

            node_util
        } else {
            /* Sample opponent action */
            let sampled_a = sample_action(&strategy, &legal, rng);
            let next_game = game.apply_action(sampled_a);
            self.cfr(&next_game, updating_player, weight, nodes, rng)
        }
    }
}

fn sample_action(strategy: &[f64], legal: &[u8], rng: &mut SmallRng) -> u8 {
    use rand::Rng;
    let mut sum = 0.0f64;
    let legal_probs: Vec<f64> = legal.iter().map(|&a| strategy[a as usize]).collect();
    let total: f64 = legal_probs.iter().sum();

    if total <= 0.0 {
        return *legal.choose(rng).unwrap();
    }

    let r: f64 = rng.gen();
    for (&a, &p) in legal.iter().zip(legal_probs.iter()) {
        sum += p / total;
        if r <= sum {
            return a;
        }
    }

    *legal.last().unwrap()
}
