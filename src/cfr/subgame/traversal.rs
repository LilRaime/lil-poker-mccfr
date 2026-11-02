/*
 * Depth-Limited CFR+ Subgame Tree Traversal (Shared Parallel & Local Single-Thread)
 * and Fast 7-Card Showdown Leaf Rollout Evaluation.
 */

use super::solver::SubgameSolver;
use crate::cfr::abstraction::{hash_holdem_infoset_key, hash_holdem_infoset_key_with_bucket};
use crate::cfr::node::InfosetNode;
use crate::game::config::*;
use crate::game::holdem::TexasHoldemGame;
use dashmap::DashMap;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use std::collections::HashMap;

impl SubgameSolver {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn cfr_shared(
        &self,
        game: &TexasHoldemGame,
        start_round: u8,
        updating_player: usize,
        weight: f64,
        precomputed_buckets: &[[u8; 2]; 3],
        nodes: &DashMap<u64, InfosetNode>,
        rng: &mut SmallRng,
    ) -> f64 {
        if game.is_terminal() {
            return game.get_returns()[updating_player];
        }

        /* Depth-limited leaf evaluation: when street depth reaches max_depth_streets,
         * evaluate the leaf directly via fast 7-card rollout showdown without expanding river trees. */
        if game.round > start_round
            && (game.round - start_round) >= self.max_depth_streets
            && game.history[(game.round - 1) as usize].is_empty()
        {
            return evaluate_leaf_rollout(game, updating_player);
        }

        let cur_p = game.current_player();
        let legal = game.legal_actions();
        if legal.is_empty() {
            return game.get_returns()[updating_player];
        }

        let key_hash = if game.round >= 2 && game.round <= 4 {
            let bucket = precomputed_buckets[(game.round - 2) as usize][cur_p];
            hash_holdem_infoset_key_with_bucket(
                &game.hole[cur_p],
                bucket,
                game.round,
                &game.history,
            )
        } else {
            hash_holdem_infoset_key(&game.hole[cur_p], &game.board, game.round, &game.history)
        };

        let strategy = if let Some(node) = nodes.get(&key_hash) {
            node.get_strategy()
        } else {
            let node = nodes
                .entry(key_hash)
                .or_insert_with(|| InfosetNode::new(HOLDEM_NUM_ACTIONS));
            node.get_strategy()
        };

        if cur_p == updating_player {
            let mut util = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut node_util = 0.0f64;

            for &a in &legal {
                let next_game = game.apply_action(a);
                let action_util = self.cfr_shared(
                    &next_game,
                    start_round,
                    updating_player,
                    weight,
                    precomputed_buckets,
                    nodes,
                    rng,
                );
                util[a as usize] = action_util;
                node_util += strategy[a as usize] * action_util;
            }

            let mut regrets = [0.0f64; HOLDEM_NUM_ACTIONS];
            for &a in &legal {
                regrets[a as usize] = util[a as usize] - node_util;
            }

            if let Some(node) = nodes.get(&key_hash) {
                node.update_regrets_cfr_plus(&regrets);
                node.accumulate_strategy(&strategy, weight);
            }

            node_util
        } else {
            /* Sample opponent action */
            let sampled_a = sample_action(&strategy, &legal, rng);
            let next_game = game.apply_action(sampled_a);
            self.cfr_shared(
                &next_game,
                start_round,
                updating_player,
                weight,
                precomputed_buckets,
                nodes,
                rng,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn cfr_local(
        &self,
        game: &TexasHoldemGame,
        start_round: u8,
        updating_player: usize,
        weight: f64,
        precomputed_buckets: &[[u8; 2]; 3],
        nodes: &mut HashMap<u64, InfosetNode>,
        rng: &mut SmallRng,
    ) -> f64 {
        if game.is_terminal() {
            return game.get_returns()[updating_player];
        }

        /* Depth-limited leaf evaluation: when street depth reaches max_depth_streets,
         * evaluate the leaf directly via fast 7-card rollout showdown without expanding river trees. */
        if game.round > start_round
            && (game.round - start_round) >= self.max_depth_streets
            && game.history[(game.round - 1) as usize].is_empty()
        {
            return evaluate_leaf_rollout(game, updating_player);
        }

        let cur_p = game.current_player();
        let legal = game.legal_actions();
        if legal.is_empty() {
            return game.get_returns()[updating_player];
        }

        let key_hash = if game.round >= 2 && game.round <= 4 {
            let bucket = precomputed_buckets[(game.round - 2) as usize][cur_p];
            hash_holdem_infoset_key_with_bucket(
                &game.hole[cur_p],
                bucket,
                game.round,
                &game.history,
            )
        } else {
            hash_holdem_infoset_key(&game.hole[cur_p], &game.board, game.round, &game.history)
        };

        let strategy = {
            let node = nodes
                .entry(key_hash)
                .or_insert_with(|| InfosetNode::new(HOLDEM_NUM_ACTIONS));
            node.get_strategy()
        };

        if cur_p == updating_player {
            let mut util = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut node_util = 0.0f64;

            for &a in &legal {
                let next_game = game.apply_action(a);
                let action_util = self.cfr_local(
                    &next_game,
                    start_round,
                    updating_player,
                    weight,
                    precomputed_buckets,
                    nodes,
                    rng,
                );
                util[a as usize] = action_util;
                node_util += strategy[a as usize] * action_util;
            }

            let mut regrets = [0.0f64; HOLDEM_NUM_ACTIONS];
            for &a in &legal {
                regrets[a as usize] = util[a as usize] - node_util;
            }

            if let Some(node) = nodes.get_mut(&key_hash) {
                node.update_regrets_cfr_plus(&regrets);
                node.accumulate_strategy(&strategy, weight);
            }

            node_util
        } else {
            /* Sample opponent action */
            let sampled_a = sample_action(&strategy, &legal, rng);
            let next_game = game.apply_action(sampled_a);
            self.cfr_local(
                &next_game,
                start_round,
                updating_player,
                weight,
                precomputed_buckets,
                nodes,
                rng,
            )
        }
    }
}

#[inline]
pub(super) fn evaluate_leaf_rollout(game: &TexasHoldemGame, updating_player: usize) -> f64 {
    let outcome = if game.showdown_outcome != i8::MIN {
        game.showdown_outcome
    } else {
        use crate::game::holdem::evaluate_7cards;
        let mut full_board = game.board;
        let mut deck_rem = game.deck_remaining;
        while full_board.len() < NUM_COMMUNITY_CARDS && !deck_rem.is_empty() {
            full_board.push(deck_rem.deal_one());
        }
        let s0 = evaluate_7cards(&game.hole[0], &full_board);
        let s1 = evaluate_7cards(&game.hole[1], &full_board);
        if s0 > s1 {
            1
        } else if s1 > s0 {
            -1
        } else {
            0
        }
    };
    let pot = game.contributions[0] + game.contributions[1];
    let ret0 = if outcome > 0 {
        (pot - game.contributions[0]) as f64
    } else if outcome < 0 {
        -(game.contributions[0] as f64)
    } else {
        0.0
    };
    if updating_player == 0 {
        ret0
    } else {
        -ret0
    }
}

pub(super) fn sample_action(strategy: &[f64], legal: &[u8], rng: &mut SmallRng) -> u8 {
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
