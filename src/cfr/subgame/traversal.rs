/*
 * Depth-Limited CFR+ Subgame Tree Traversal (Shared Parallel & Local Single-Thread)
 * and Fast 7-Card Showdown Leaf Rollout Evaluation.
 */

use super::solver::SubgameSolver;
use crate::cfr::abstraction::{hash_holdem_infoset_key, hash_holdem_infoset_key_with_bucket};
use crate::cfr::fallback::{get_holdem_fallback_strategy, get_postflop_fallback_by_bucket};
use crate::cfr::node::InfosetNode;
use crate::game::config::*;
use crate::game::holdem::TexasHoldemGame;
use dashmap::DashMap;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use std::collections::HashMap;

/* Light regret-matching prior weight for newly expanded subgame child nodes.
 * Strong enough to steer initial opponent/range play away from uniform 1/6 noise,
 * yet small enough that CFR+ chip regrets override it within a few iterations. */
const CHILD_PRIOR_REGRET_WEIGHT: f64 = 25.0;

#[inline]
fn compute_subgame_key_hash(
    game: &TexasHoldemGame,
    start_round: u8,
    cur_p: usize,
    precomputed_buckets: &[[u8; 2]; 3],
) -> u64 {
    let mut h = if game.round >= 2 && game.round <= 4 {
        let bucket = precomputed_buckets[(game.round - 2) as usize][cur_p];
        hash_holdem_infoset_key_with_bucket(&game.hole[cur_p], bucket, game.round, &game.history)
    } else {
        hash_holdem_infoset_key(&game.hole[cur_p], &game.board, game.round, &game.history)
    };
    /* Mix prior street actions on deeper streets of 2-street subgames so distinct branches
     * (e.g. Flop c-c vs Flop r-c leading into the same Turn) never alias in the node table. */
    if game.round > start_round && (1..=4).contains(&start_round) {
        let prev_acts = game.history[(start_round - 1) as usize].as_slice();
        for &a in prev_acts {
            h ^= (a as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
    }
    h
}

#[inline]
fn normalize_legal_strategy(raw: &[f64], legal: &[u8]) -> [f64; HOLDEM_NUM_ACTIONS] {
    let mut norm = [0.0f64; HOLDEM_NUM_ACTIONS];
    let mut sum = 0.0f64;
    for &a in legal {
        let idx = a as usize;
        let p = raw.get(idx).copied().unwrap_or(0.0).max(0.0);
        norm[idx] = p;
        sum += p;
    }
    if sum > 1e-12 {
        let inv = 1.0 / sum;
        for &a in legal {
            norm[a as usize] *= inv;
        }
    } else if !legal.is_empty() {
        let u = 1.0 / legal.len() as f64;
        for &a in legal {
            norm[a as usize] = u;
        }
    }
    norm
}

#[inline]
pub(super) fn create_prior_infoset_node(
    game: &TexasHoldemGame,
    cur_p: usize,
    my_player: usize,
    opp_prior_bias: Option<(f64, f64)>,
    legal: &[u8],
    precomputed_buckets: &[[u8; 2]; 3],
) -> InfosetNode {
    let opp = 1 - cur_p;
    let to_call = (game.contributions[opp] - game.contributions[cur_p]).max(0);
    let pot = game.contributions[0] + game.contributions[1];
    let mut prior = if game.round >= 2 && game.round <= 4 {
        let bucket = precomputed_buckets[(game.round - 2) as usize][cur_p] as usize;
        get_postflop_fallback_by_bucket(bucket, game.round, to_call, pot, legal)
    } else {
        get_holdem_fallback_strategy(
            &game.hole[cur_p],
            &game.board,
            game.round,
            to_call,
            pot,
            legal,
        )
    };
    /* Stat-informed opponent node-locking prior bias */
    if cur_p != my_player {
        if let Some((fold_m, raise_m)) = opp_prior_bias {
            prior[FOLD as usize] *= fold_m;
            for r in RAISE_MIN..=ALL_IN {
                prior[r as usize] *= raise_m;
            }
            let s: f64 = legal.iter().map(|&a| prior[a as usize]).sum();
            if s > 1e-9 {
                for &a in legal {
                    prior[a as usize] /= s;
                }
            }
        }
    }
    let node = InfosetNode::new(HOLDEM_NUM_ACTIONS);
    let mut init_regrets = [0.0f64; HOLDEM_NUM_ACTIONS];
    for i in 0..HOLDEM_NUM_ACTIONS {
        init_regrets[i] = prior[i] * CHILD_PRIOR_REGRET_WEIGHT;
    }
    node.update_regrets_cfr_plus(&init_regrets);
    node
}

impl SubgameSolver {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn cfr_shared(
        &self,
        game: &TexasHoldemGame,
        start_round: u8,
        root_hash: u64,
        my_player: usize,
        opp_prior_bias: Option<(f64, f64)>,
        updating_player: usize,
        weight: f64,
        precomputed_buckets: &[[u8; 2]; 3],
        nodes: &DashMap<u64, InfosetNode>,
        rng: &mut SmallRng,
    ) -> f64 {
        if game.is_terminal() {
            return game.get_returns()[updating_player];
        }

        /* Depth-limited continuation leaf evaluation */
        if game.round > start_round
            && (game.round - start_round) >= self.max_depth_streets
            && game.history[(game.round - 1) as usize].is_empty()
        {
            return evaluate_leaf_continuation(game, updating_player, precomputed_buckets);
        }

        let cur_p = game.current_player();
        let legal = game.legal_actions();
        if legal.is_empty() {
            return game.get_returns()[updating_player];
        }

        let key_hash = compute_subgame_key_hash(game, start_round, cur_p, precomputed_buckets);

        let raw_strategy = if let Some(node) = nodes.get(&key_hash) {
            node.get_strategy()
        } else {
            let node = nodes.entry(key_hash).or_insert_with(|| {
                create_prior_infoset_node(
                    game,
                    cur_p,
                    my_player,
                    opp_prior_bias,
                    &legal,
                    precomputed_buckets,
                )
            });
            node.get_strategy()
        };
        let strategy = normalize_legal_strategy(&raw_strategy, &legal);

        if cur_p == updating_player {
            let mut util = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut node_util = 0.0f64;

            for &a in &legal {
                let next_game = game.apply_action(a);
                let action_util = self.cfr_shared(
                    &next_game,
                    start_round,
                    root_hash,
                    my_player,
                    opp_prior_bias,
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
                if key_hash == root_hash {
                    node.accumulate_strategy(&strategy, weight);
                }
            }

            node_util
        } else {
            if key_hash != root_hash {
                if let Some(node) = nodes.get(&key_hash) {
                    node.accumulate_strategy(&strategy, weight);
                }
            }
            let sampled_a = sample_action(&strategy, &legal, rng);
            let next_game = game.apply_action(sampled_a);
            self.cfr_shared(
                &next_game,
                start_round,
                root_hash,
                my_player,
                opp_prior_bias,
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
        root_hash: u64,
        my_player: usize,
        opp_prior_bias: Option<(f64, f64)>,
        updating_player: usize,
        weight: f64,
        precomputed_buckets: &[[u8; 2]; 3],
        nodes: &mut HashMap<u64, InfosetNode>,
        rng: &mut SmallRng,
    ) -> f64 {
        if game.is_terminal() {
            return game.get_returns()[updating_player];
        }

        /* Depth-limited continuation leaf evaluation */
        if game.round > start_round
            && (game.round - start_round) >= self.max_depth_streets
            && game.history[(game.round - 1) as usize].is_empty()
        {
            return evaluate_leaf_continuation(game, updating_player, precomputed_buckets);
        }

        let cur_p = game.current_player();
        let legal = game.legal_actions();
        if legal.is_empty() {
            return game.get_returns()[updating_player];
        }

        let key_hash = compute_subgame_key_hash(game, start_round, cur_p, precomputed_buckets);

        let raw_strategy = {
            let node = nodes.entry(key_hash).or_insert_with(|| {
                create_prior_infoset_node(
                    game,
                    cur_p,
                    my_player,
                    opp_prior_bias,
                    &legal,
                    precomputed_buckets,
                )
            });
            node.get_strategy()
        };
        let strategy = normalize_legal_strategy(&raw_strategy, &legal);

        if cur_p == updating_player {
            let mut util = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut node_util = 0.0f64;

            for &a in &legal {
                let next_game = game.apply_action(a);
                let action_util = self.cfr_local(
                    &next_game,
                    start_round,
                    root_hash,
                    my_player,
                    opp_prior_bias,
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
                if key_hash == root_hash {
                    node.accumulate_strategy(&strategy, weight);
                }
            }

            node_util
        } else {
            if key_hash != root_hash {
                if let Some(node) = nodes.get_mut(&key_hash) {
                    node.accumulate_strategy(&strategy, weight);
                }
            }
            let sampled_a = sample_action(&strategy, &legal, rng);
            let next_game = game.apply_action(sampled_a);
            self.cfr_local(
                &next_game,
                start_round,
                root_hash,
                my_player,
                opp_prior_bias,
                updating_player,
                weight,
                precomputed_buckets,
                nodes,
                rng,
            )
        }
    }
}

/* Continuation leaf evaluation: combines 7-card showdown rollout with a realizable future-street
 * value-extraction continuation term (inspired by Pluribus continuation leaves) so strong hands
 * with remaining stack behind do not undervalue future-street implied EV. */
#[inline]
pub(super) fn evaluate_leaf_continuation(
    game: &TexasHoldemGame,
    updating_player: usize,
    precomputed_buckets: &[[u8; 2]; 3],
) -> f64 {
    let base_ev = evaluate_leaf_rollout(game, updating_player);
    let max_c = game.contributions[0].max(game.contributions[1]);
    let eff_rem = (game.stack_size - max_c).max(0) as f64;
    if eff_rem <= 1.0 || game.round < 2 || game.round > 4 {
        return base_ev;
    }
    let pot = (game.contributions[0] + game.contributions[1]) as f64;
    let street_idx = (game.round - 2) as usize;
    let b0 = precomputed_buckets[street_idx][0];
    let b1 = precomputed_buckets[street_idx][1];
    /* Value-betting continuation bonus: when one player holds 2P+ (>= 57) and the other holds
     * a bluff-catcher/pair (28..=56), the stronger hand extracts additional chips on average. */
    let cont_scale = (pot * 0.20).min(eff_rem * 0.35);
    let cont_p0 = if b0 >= 57 && (28..=56).contains(&b1) {
        cont_scale
    } else if b1 >= 57 && (28..=56).contains(&b0) {
        -cont_scale
    } else {
        0.0
    };
    let cont = if updating_player == 0 {
        cont_p0
    } else {
        -cont_p0
    };
    base_ev + cont
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
