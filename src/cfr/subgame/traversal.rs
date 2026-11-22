/*!
 * Depth-Limited CFR+ Subgame Tree Traversal (Shared Parallel & Local Single-Thread)
 * and Fast 7-Card Showdown Leaf Rollout Evaluation.
 */

use super::solver::SubgameSolver;
use crate::cfr::abstraction::{
    BUCKET_TRIPS_MIN, BUCKET_TWO_PAIR_MIN, BUCKET_WEAK_PAIR_MIN, hash_holdem_infoset_key,
    hash_holdem_infoset_key_with_bucket,
};
use crate::cfr::fallback::{
    FallbackRequest, get_holdem_fallback_strategy, get_postflop_fallback_by_bucket,
};
use crate::cfr::node::InfosetNode;
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, HOLDEM_NUM_ACTIONS, NUM_COMMUNITY_CARDS, RAISE_MIN, ROUND_FLOP,
    ROUND_PREFLOP, ROUND_RIVER,
};
use crate::game::holdem::TexasHoldemGame;
use dashmap::DashMap;
use rand::RngExt;
use rand::rngs::SmallRng;
use rand::seq::IndexedRandom;
use std::collections::HashMap;

/** Light regret-matching prior weight for newly expanded subgame child nodes.
 * Strong enough to steer initial opponent/range play away from uniform 1/6 noise,
 * yet small enough that CFR+ chip regrets override it within a few iterations. */
const CHILD_PRIOR_REGRET_WEIGHT: f64 = 25.0;

#[inline]
fn compute_subgame_key_hash(
    game: &TexasHoldemGame,
    start_round: u8,
    cur_p: usize,
    precomputed_buckets: [[u8; 2]; 3],
) -> u64 {
    let mut h = if (ROUND_FLOP..=ROUND_RIVER).contains(&game.round) {
        let bucket = precomputed_buckets[(game.round - 2) as usize][cur_p];
        hash_holdem_infoset_key_with_bucket(&game.hole[cur_p], bucket, game.round, &game.history)
    } else {
        hash_holdem_infoset_key(&game.hole[cur_p], &game.board, game.round, &game.history)
    };
    /* Mix prior street actions on deeper streets of 2-street subgames so distinct branches
     * (e.g. Flop c-c vs Flop r-c leading into the same Turn) never alias in the node table. */
    if game.round > start_round && (ROUND_PREFLOP..=ROUND_RIVER).contains(&start_round) {
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
pub(super) fn filter_subgame_legal_actions(game: &TexasHoldemGame, cur_p: usize) -> Vec<u8> {
    let mut legal = game.legal_actions();
    if legal.contains(&ALL_IN) && legal.iter().any(|&a| (RAISE_MIN..ALL_IN).contains(&a)) {
        let opp = 1 - cur_p;
        let to_call = (game.contributions[opp] - game.contributions[cur_p]).max(0);
        let pot = (game.contributions[0] + game.contributions[1]).max(1);
        let rem_stack = (game.stack_size - game.contributions[cur_p]).max(0);
        let rem_after_call = (rem_stack - to_call).max(0);
        let pot_after_call = pot + to_call;
        if rem_after_call > pot_after_call * 3 {
            legal.retain(|&a| a != ALL_IN);
        }
    }
    legal
}

#[inline]
pub(super) fn create_prior_infoset_node(
    game: &TexasHoldemGame,
    cur_p: usize,
    my_player: usize,
    opp_prior_bias: Option<(f64, f64)>,
    legal: &[u8],
    precomputed_buckets: [[u8; 2]; 3],
) -> InfosetNode {
    let opp = 1 - cur_p;
    let to_call = (game.contributions[opp] - game.contributions[cur_p]).max(0);
    let pot = game.contributions[0] + game.contributions[1];
    let postflop_bucket = if (ROUND_FLOP..=ROUND_RIVER).contains(&game.round) {
        Some(precomputed_buckets[(game.round - 2) as usize][cur_p] as usize)
    } else {
        None
    };
    let mut prior = if let Some(bucket) = postflop_bucket {
        get_postflop_fallback_by_bucket(bucket, game.round, to_call, pot, legal)
    } else {
        get_holdem_fallback_strategy(&FallbackRequest {
            hole: game.hole[cur_p],
            board: &game.board,
            round: game.round,
            to_call,
            pot,
            legal,
            ..Default::default()
        })
    };
    /* Dampen call/raise prior weights when facing >2x pot jams with non-trips hands
     * so SubgameSolver does not hallucinate call-EV for massive overbet shoves. */
    if let Some(bucket) = postflop_bucket
        && to_call > 0
        && legal.contains(&FOLD)
        && bucket < BUCKET_TRIPS_MIN
    {
        let pre_bet_pot = (pot - to_call).max(1);
        if to_call > pre_bet_pot * 2 {
            let mut added_fold = 0.0f64;
            if legal.contains(&CALL_CHECK) {
                let old_c = prior[CALL_CHECK as usize];
                let new_c = old_c * 0.35;
                added_fold += old_c - new_c;
                prior[CALL_CHECK as usize] = new_c;
            }
            for r in RAISE_MIN..=ALL_IN {
                if legal.contains(&r) {
                    let old_r = prior[r as usize];
                    let new_r = old_r * 0.25;
                    added_fold += old_r - new_r;
                    prior[r as usize] = new_r;
                }
            }
            prior[FOLD as usize] += added_fold;
            let s: f64 = legal.iter().map(|&a| prior[a as usize]).sum();
            if s > 1e-9 {
                for &a in legal {
                    prior[a as usize] /= s;
                }
            }
        }
    }
    /* Stat-informed opponent node-locking prior bias */
    if cur_p != my_player
        && let Some((fold_m, raise_m)) = opp_prior_bias
    {
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
    let node = InfosetNode::new(HOLDEM_NUM_ACTIONS);
    let mut init_regrets = [0.0f64; HOLDEM_NUM_ACTIONS];
    for i in 0..HOLDEM_NUM_ACTIONS {
        init_regrets[i] = prior[i] * CHILD_PRIOR_REGRET_WEIGHT;
    }
    node.update_regrets_cfr_plus(&init_regrets);
    node
}

pub(super) struct TraversalCtx {
    pub start_round: u8,
    pub root_hash: u64,
    pub my_player: usize,
    pub opp_prior_bias: Option<(f64, f64)>,
    pub updating_player: usize,
    pub weight: f64,
    pub precomputed_buckets: [[u8; 2]; 3],
}

impl SubgameSolver {
    pub(super) fn cfr_shared(
        &self,
        game: &TexasHoldemGame,
        ctx: &TraversalCtx,
        nodes: &DashMap<u64, InfosetNode>,
        rng: &mut SmallRng,
    ) -> f64 {
        if game.is_terminal() {
            return game.get_returns()[ctx.updating_player];
        }

        /* Depth-limited continuation leaf evaluation */
        if game.round > ctx.start_round
            && (game.round - ctx.start_round) >= self.max_depth_streets
            && game.history[(game.round - 1) as usize].is_empty()
        {
            return evaluate_leaf_continuation(game, ctx.updating_player, ctx.precomputed_buckets);
        }

        let cur_p = game.current_player();
        let legal = filter_subgame_legal_actions(game, cur_p);
        if legal.is_empty() {
            return game.get_returns()[ctx.updating_player];
        }

        let key_hash =
            compute_subgame_key_hash(game, ctx.start_round, cur_p, ctx.precomputed_buckets);

        let raw_strategy = if let Some(node) = nodes.get(&key_hash) {
            node.get_strategy()
        } else {
            let node = nodes.entry(key_hash).or_insert_with(|| {
                create_prior_infoset_node(
                    game,
                    cur_p,
                    ctx.my_player,
                    ctx.opp_prior_bias,
                    &legal,
                    ctx.precomputed_buckets,
                )
            });
            node.get_strategy()
        };
        let strategy = normalize_legal_strategy(&raw_strategy, &legal);

        if cur_p == ctx.updating_player {
            let mut util = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut node_util = 0.0f64;

            for &a in &legal {
                let next_game = game.apply_action(a);
                let action_util = self.cfr_shared(&next_game, ctx, nodes, rng);
                util[a as usize] = action_util;
                node_util += strategy[a as usize] * action_util;
            }

            let mut regrets = [0.0f64; HOLDEM_NUM_ACTIONS];
            for &a in &legal {
                regrets[a as usize] = util[a as usize] - node_util;
            }

            if let Some(node) = nodes.get(&key_hash) {
                node.update_regrets_cfr_plus(&regrets);
                if key_hash == ctx.root_hash {
                    node.accumulate_strategy(&strategy, ctx.weight);
                }
            }

            node_util
        } else {
            if key_hash != ctx.root_hash
                && let Some(node) = nodes.get(&key_hash)
            {
                node.accumulate_strategy(&strategy, ctx.weight);
            }
            let sampled_a = sample_action(&strategy, &legal, rng);
            let next_game = game.apply_action(sampled_a);
            self.cfr_shared(&next_game, ctx, nodes, rng)
        }
    }

    pub(super) fn cfr_local(
        &self,
        game: &TexasHoldemGame,
        ctx: &TraversalCtx,
        nodes: &mut HashMap<u64, InfosetNode>,
        rng: &mut SmallRng,
    ) -> f64 {
        if game.is_terminal() {
            return game.get_returns()[ctx.updating_player];
        }

        /* Depth-limited continuation leaf evaluation */
        if game.round > ctx.start_round
            && (game.round - ctx.start_round) >= self.max_depth_streets
            && game.history[(game.round - 1) as usize].is_empty()
        {
            return evaluate_leaf_continuation(game, ctx.updating_player, ctx.precomputed_buckets);
        }

        let cur_p = game.current_player();
        let legal = filter_subgame_legal_actions(game, cur_p);
        if legal.is_empty() {
            return game.get_returns()[ctx.updating_player];
        }

        let key_hash =
            compute_subgame_key_hash(game, ctx.start_round, cur_p, ctx.precomputed_buckets);

        let raw_strategy = {
            let node = nodes.entry(key_hash).or_insert_with(|| {
                create_prior_infoset_node(
                    game,
                    cur_p,
                    ctx.my_player,
                    ctx.opp_prior_bias,
                    &legal,
                    ctx.precomputed_buckets,
                )
            });
            node.get_strategy()
        };
        let strategy = normalize_legal_strategy(&raw_strategy, &legal);

        if cur_p == ctx.updating_player {
            let mut util = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut node_util = 0.0f64;

            for &a in &legal {
                let next_game = game.apply_action(a);
                let action_util = self.cfr_local(&next_game, ctx, nodes, rng);
                util[a as usize] = action_util;
                node_util += strategy[a as usize] * action_util;
            }

            let mut regrets = [0.0f64; HOLDEM_NUM_ACTIONS];
            for &a in &legal {
                regrets[a as usize] = util[a as usize] - node_util;
            }

            if let Some(node) = nodes.get_mut(&key_hash) {
                node.update_regrets_cfr_plus(&regrets);
                if key_hash == ctx.root_hash {
                    node.accumulate_strategy(&strategy, ctx.weight);
                }
            }

            node_util
        } else {
            if key_hash != ctx.root_hash
                && let Some(node) = nodes.get_mut(&key_hash)
            {
                node.accumulate_strategy(&strategy, ctx.weight);
            }
            let sampled_a = sample_action(&strategy, &legal, rng);
            let next_game = game.apply_action(sampled_a);
            self.cfr_local(&next_game, ctx, nodes, rng)
        }
    }
}

/** Continuation leaf evaluation: combines 7-card showdown rollout with a realizable future-street
 * value-extraction continuation term (inspired by Pluribus continuation leaves) so strong hands
 * with remaining stack behind do not undervalue future-street implied EV. */
#[inline]
pub(super) fn evaluate_leaf_continuation(
    game: &TexasHoldemGame,
    updating_player: usize,
    precomputed_buckets: [[u8; 2]; 3],
) -> f64 {
    let base_ev = evaluate_leaf_rollout(game, updating_player);
    let max_c = game.contributions[0].max(game.contributions[1]);
    let eff_rem = (game.stack_size - max_c).max(0) as f64;
    if eff_rem <= 1.0 || game.round < ROUND_FLOP || game.round > ROUND_RIVER {
        return base_ev;
    }
    let pot = (game.contributions[0] + game.contributions[1]) as f64;
    let street_idx = (game.round - 2) as usize;
    let b0 = precomputed_buckets[street_idx][0] as usize;
    let b1 = precomputed_buckets[street_idx][1] as usize;
    /* Value-betting continuation bonus: when one player holds 2P+ (>= 57) and the other holds
     * a bluff-catcher/pair (28..=56), the stronger hand extracts additional chips on average. */
    let cont_scale = (pot * 0.20).min(eff_rem * 0.35);
    let cont_p0 = if b0 >= BUCKET_TWO_PAIR_MIN
        && (BUCKET_WEAK_PAIR_MIN..BUCKET_TWO_PAIR_MIN).contains(&b1)
    {
        cont_scale
    } else if b1 >= BUCKET_TWO_PAIR_MIN && (BUCKET_WEAK_PAIR_MIN..BUCKET_TWO_PAIR_MIN).contains(&b0)
    {
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
    if updating_player == 0 { ret0 } else { -ret0 }
}

pub(super) fn sample_action(strategy: &[f64], legal: &[u8], rng: &mut SmallRng) -> u8 {
    let mut sum = 0.0f64;
    let legal_probs: Vec<f64> = legal.iter().map(|&a| strategy[a as usize]).collect();
    let total: f64 = legal_probs.iter().sum();

    if total <= 0.0 {
        return legal.choose(rng).copied().unwrap_or(CALL_CHECK);
    }

    let r: f64 = rng.random();
    for (&a, &p) in legal.iter().zip(legal_probs.iter()) {
        sum += p / total;
        if r <= sum {
            return a;
        }
    }

    legal.last().copied().unwrap_or(CALL_CHECK)
}
