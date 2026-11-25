/*!
 * Depth-Limited CFR+ Subgame Tree Traversal (Shared Parallel & Local Single-Thread)
 * and Fast 7-Card Showdown Leaf Rollout Evaluation.
 */

use super::solver::{OppStreetProfile, SubgameSolver};
use crate::cfr::abstraction::{
    BUCKET_RIVER_NUT_FLUSH_BLOCKER_AIR, BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_AIR,
    BUCKET_STRONG_DRAW_MIN, BUCKET_TOP_PAIR_MIN, BUCKET_TRIPS_MIN, BUCKET_TWO_PAIR_MIN,
    BUCKET_WEAK_PAIR_MIN, hash_holdem_infoset_key, hash_holdem_infoset_key_with_bucket,
};
use crate::cfr::fallback::{
    FallbackRequest, get_holdem_fallback_strategy, get_postflop_fallback_by_bucket,
};
use crate::cfr::node::{InfosetNode, MAX_ACTIONS};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, HOLDEM_NUM_ACTIONS, NUM_COMMUNITY_CARDS, RAISE_MIN, ROUND_FLOP,
    ROUND_PREFLOP, ROUND_RIVER, ROUND_TURN,
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

/** Number of Pluribus-style continuation policies at depth-limited Flop/Turn leaves. */
pub(super) const NUM_CONTINUATION_POLICIES: usize = 4;
const POLICY_BALANCED: usize = 0;
const POLICY_TIGHT_FOLD: usize = 1;
const POLICY_STATION_CALL: usize = 2;
const POLICY_AGGRESSIVE: usize = 3;
const LEAF_POLICY_HASH_SALT: u64 = 0x4C45_4146_504F_4C59;

#[inline]
fn compute_subgame_key_hash(
    game: &TexasHoldemGame,
    start_round: u8,
    cur_p: usize,
    precomputed_buckets: [[u8; 2]; 3],
    precomputed_ext_river: [u8; 2],
) -> u64 {
    let mut h = if game.round == ROUND_RIVER {
        let bucket = precomputed_ext_river[cur_p];
        hash_holdem_infoset_key_with_bucket(&game.hole[cur_p], bucket, game.round, &game.history)
    } else if (ROUND_FLOP..ROUND_RIVER).contains(&game.round) {
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
fn compute_leaf_policy_hash(
    game: &TexasHoldemGame,
    start_round: u8,
    player: usize,
    precomputed_buckets: [[u8; 2]; 3],
    precomputed_ext_river: [u8; 2],
) -> u64 {
    compute_subgame_key_hash(
        game,
        start_round,
        player,
        precomputed_buckets,
        precomputed_ext_river,
    ) ^ LEAF_POLICY_HASH_SALT
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
fn compute_opponent_node_lock_multipliers(
    game: &TexasHoldemGame,
    to_call: i32,
    postflop_bucket: Option<usize>,
    opp_prior_bias: Option<(f64, f64)>,
    opp_profile: Option<OppStreetProfile>,
) -> (f64, f64) {
    let (mut fold_m, mut raise_m) = opp_prior_bias.unwrap_or((1.0, 1.0));
    if let Some(prof) = opp_profile {
        if to_call > 0 {
            let street_fold = match game.round {
                ROUND_FLOP => prof.flop_fold_to_cbet.map(|f| (f / 0.42).clamp(0.55, 1.85)),
                ROUND_TURN => prof
                    .turn_fold_to_barrel
                    .map(|f| (f / 0.40).clamp(0.55, 1.85)),
                ROUND_RIVER => prof.river_fold_to_bet.map(|f| (f / 0.38).clamp(0.55, 1.90)),
                _ => None,
            };
            if let Some(sf) = street_fold {
                fold_m = sf;
            }
        }
        if let Some(r_rate) = prof.postflop_raise_rate {
            let agg_m = (r_rate / 0.22).clamp(0.60, 1.65);
            raise_m = if opp_prior_bias.is_some() {
                (raise_m + agg_m) * 0.5
            } else {
                agg_m
            };
        }
        if prof.overbet_freq.is_some_and(|f| f >= 0.35) {
            raise_m = raise_m.clamp(1.35, 1.80);
        }
        if game.round == ROUND_RIVER
            && postflop_bucket.is_some_and(|b| b < BUCKET_WEAK_PAIR_MIN)
            && prof.overbet_value_rate.is_some_and(|vr| vr >= 0.75)
        {
            raise_m *= 0.65;
        }
    }
    (fold_m, raise_m)
}

#[inline]
fn apply_opponent_node_lock(
    strategy: &mut [f64; HOLDEM_NUM_ACTIONS],
    game: &TexasHoldemGame,
    cur_p: usize,
    ctx: &TraversalCtx,
    legal: &[u8],
) {
    if cur_p == ctx.my_player || (ctx.opp_prior_bias.is_none() && ctx.opp_profile.is_none()) {
        return;
    }
    let opp = 1 - cur_p;
    let to_call = (game.contributions[opp] - game.contributions[cur_p]).max(0);
    let postflop_bucket = if (ROUND_FLOP..=ROUND_RIVER).contains(&game.round) {
        Some(ctx.precomputed_buckets[(game.round - 2) as usize][cur_p] as usize)
    } else {
        None
    };
    let (fold_m, raise_m) = compute_opponent_node_lock_multipliers(
        game,
        to_call,
        postflop_bucket,
        ctx.opp_prior_bias,
        ctx.opp_profile,
    );
    if (fold_m - 1.0).abs() <= 1e-6 && (raise_m - 1.0).abs() <= 1e-6 {
        return;
    }

    if to_call > 0 && legal.contains(&FOLD) && legal.contains(&CALL_CHECK) {
        if fold_m > 1.10 && postflop_bucket.is_none_or(|b| b < BUCKET_TWO_PAIR_MIN) {
            let shift_frac = ((fold_m - 1.0) * 0.45).clamp(0.0, 0.55);
            let shifted = strategy[CALL_CHECK as usize] * shift_frac;
            strategy[CALL_CHECK as usize] -= shifted;
            strategy[FOLD as usize] += shifted;
        } else if fold_m < 0.90 {
            let shift_frac = ((1.0 - fold_m) * 0.60).clamp(0.0, 0.45);
            let shifted = strategy[FOLD as usize] * shift_frac;
            strategy[FOLD as usize] -= shifted;
            strategy[CALL_CHECK as usize] += shifted;
        }
    }
    if to_call > 0 {
        strategy[FOLD as usize] *= fold_m;
    }
    for r in RAISE_MIN..=ALL_IN {
        strategy[r as usize] *= raise_m;
    }
    let s: f64 = legal.iter().map(|&a| strategy[a as usize]).sum();
    if s > 1e-9 {
        let inv = 1.0 / s;
        for &a in legal {
            strategy[a as usize] *= inv;
        }
    }
}

#[inline]
pub(super) fn create_prior_infoset_node(
    game: &TexasHoldemGame,
    cur_p: usize,
    ctx: &TraversalCtx,
    legal: &[u8],
) -> InfosetNode {
    let my_player = ctx.my_player;
    let opp_prior_bias = ctx.opp_prior_bias;
    let opp_profile = ctx.opp_profile;
    let precomputed_buckets = ctx.precomputed_buckets;
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
    /* Stat-informed opponent street-level node-locking prior bias */
    if cur_p != my_player {
        let (fold_m, raise_m) = compute_opponent_node_lock_multipliers(
            game,
            to_call,
            postflop_bucket,
            opp_prior_bias,
            opp_profile,
        );
        if (fold_m - 1.0).abs() > 1e-6 || (raise_m - 1.0).abs() > 1e-6 {
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

pub(super) struct TraversalCtx {
    pub start_round: u8,
    pub effective_max_depth: u8,
    pub root_hash: u64,
    pub my_player: usize,
    pub opp_prior_bias: Option<(f64, f64)>,
    pub opp_profile: Option<OppStreetProfile>,
    pub updating_player: usize,
    pub weight: f64,
    pub precomputed_buckets: [[u8; 2]; 3],
    pub precomputed_ext_river: [u8; 2],
}

#[inline]
fn create_leaf_policy_node(
    ctx: &TraversalCtx,
    game: &TexasHoldemGame,
    player: usize,
) -> InfosetNode {
    let mut priors = [0.20f64; NUM_CONTINUATION_POLICIES];
    priors[POLICY_BALANCED] = 0.40;
    if player != ctx.my_player {
        let (fold_m, raise_m) = compute_opponent_node_lock_multipliers(
            game,
            1,
            None,
            ctx.opp_prior_bias,
            ctx.opp_profile,
        );
        if fold_m > 1.15 {
            priors[POLICY_TIGHT_FOLD] += 0.35;
        } else if fold_m < 0.85 {
            priors[POLICY_STATION_CALL] += 0.35;
        }
        if raise_m > 1.20 {
            priors[POLICY_AGGRESSIVE] += 0.35;
        }
    }
    let sum: f64 = priors.iter().sum();
    let mut init_regrets = [0.0f64; MAX_ACTIONS];
    for i in 0..NUM_CONTINUATION_POLICIES {
        init_regrets[i] = (priors[i] / sum) * CHILD_PRIOR_REGRET_WEIGHT;
    }
    let node = InfosetNode::new(NUM_CONTINUATION_POLICIES);
    node.update_regrets_cfr_plus(&init_regrets[..NUM_CONTINUATION_POLICIES]);
    node
}

#[inline(always)]
fn sample_policy_4(strat: &[f64; MAX_ACTIONS], rng: &mut SmallRng) -> usize {
    let r: f64 = rng.random();
    let mut cum = 0.0f64;
    for (i, &p) in strat.iter().enumerate().take(NUM_CONTINUATION_POLICIES) {
        cum += p;
        if r <= cum {
            return i;
        }
    }
    NUM_CONTINUATION_POLICIES - 1
}

#[inline]
fn evaluate_policy_best_response(
    game: &TexasHoldemGame,
    me: usize,
    opp_pol: usize,
    my_strat: &[f64; MAX_ACTIONS],
    ctx: &TraversalCtx,
) -> (f64, [f64; NUM_CONTINUATION_POLICIES]) {
    let mut util = [0.0f64; NUM_CONTINUATION_POLICIES];
    let mut node_util = 0.0f64;
    for my_pol in 0..NUM_CONTINUATION_POLICIES {
        let (p0_pol, p1_pol) = if me == 0 {
            (my_pol, opp_pol)
        } else {
            (opp_pol, my_pol)
        };
        let u = evaluate_continuation_leaf_payoff(
            game,
            me,
            ctx.precomputed_buckets,
            ctx.precomputed_ext_river,
            p0_pol,
            p1_pol,
        );
        util[my_pol] = u;
        node_util += my_strat[my_pol] * u;
    }
    let mut regrets = [0.0f64; NUM_CONTINUATION_POLICIES];
    for i in 0..NUM_CONTINUATION_POLICIES {
        regrets[i] = util[i] - node_util;
    }
    (node_util, regrets)
}

impl SubgameSolver {
    fn eval_multi_policy_leaf_shared(
        &self,
        game: &TexasHoldemGame,
        ctx: &TraversalCtx,
        nodes: &DashMap<u64, InfosetNode>,
        rng: &mut SmallRng,
    ) -> f64 {
        let me = ctx.updating_player;
        let opp = 1 - me;
        let opp_hash = compute_leaf_policy_hash(
            game,
            ctx.start_round,
            opp,
            ctx.precomputed_buckets,
            ctx.precomputed_ext_river,
        );
        let mut opp_strat = [0.0f64; MAX_ACTIONS];
        if let Some(node) = nodes.get(&opp_hash) {
            node.get_strategy_buf(&mut opp_strat);
            node.accumulate_strategy(&opp_strat[..NUM_CONTINUATION_POLICIES], ctx.weight);
        } else {
            let node = nodes
                .entry(opp_hash)
                .or_insert_with(|| create_leaf_policy_node(ctx, game, opp));
            node.get_strategy_buf(&mut opp_strat);
            node.accumulate_strategy(&opp_strat[..NUM_CONTINUATION_POLICIES], ctx.weight);
        }
        let opp_pol = sample_policy_4(&opp_strat, rng);

        let my_hash = compute_leaf_policy_hash(
            game,
            ctx.start_round,
            me,
            ctx.precomputed_buckets,
            ctx.precomputed_ext_river,
        );
        let mut my_strat = [0.0f64; MAX_ACTIONS];
        if let Some(node) = nodes.get(&my_hash) {
            node.get_strategy_buf(&mut my_strat);
        } else {
            let node = nodes
                .entry(my_hash)
                .or_insert_with(|| create_leaf_policy_node(ctx, game, me));
            node.get_strategy_buf(&mut my_strat);
        }

        let (node_util, regrets) = evaluate_policy_best_response(game, me, opp_pol, &my_strat, ctx);
        if let Some(node) = nodes.get(&my_hash) {
            node.update_regrets_cfr_plus(&regrets);
        }
        node_util
    }

    fn eval_multi_policy_leaf_local(
        &self,
        game: &TexasHoldemGame,
        ctx: &TraversalCtx,
        nodes: &mut HashMap<u64, InfosetNode>,
        rng: &mut SmallRng,
    ) -> f64 {
        let me = ctx.updating_player;
        let opp = 1 - me;
        let opp_hash = compute_leaf_policy_hash(
            game,
            ctx.start_round,
            opp,
            ctx.precomputed_buckets,
            ctx.precomputed_ext_river,
        );
        let mut opp_strat = [0.0f64; MAX_ACTIONS];
        {
            let node = nodes
                .entry(opp_hash)
                .or_insert_with(|| create_leaf_policy_node(ctx, game, opp));
            node.get_strategy_buf(&mut opp_strat);
            node.accumulate_strategy(&opp_strat[..NUM_CONTINUATION_POLICIES], ctx.weight);
        }
        let opp_pol = sample_policy_4(&opp_strat, rng);

        let my_hash = compute_leaf_policy_hash(
            game,
            ctx.start_round,
            me,
            ctx.precomputed_buckets,
            ctx.precomputed_ext_river,
        );
        let mut my_strat = [0.0f64; MAX_ACTIONS];
        {
            let node = nodes
                .entry(my_hash)
                .or_insert_with(|| create_leaf_policy_node(ctx, game, me));
            node.get_strategy_buf(&mut my_strat);
        }

        let (node_util, regrets) = evaluate_policy_best_response(game, me, opp_pol, &my_strat, ctx);
        if let Some(node) = nodes.get_mut(&my_hash) {
            node.update_regrets_cfr_plus(&regrets);
        }
        node_util
    }

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

        /* Depth-limited Pluribus-style multi-policy continuation leaf evaluation */
        if game.round > ctx.start_round
            && (game.round - ctx.start_round) >= ctx.effective_max_depth
            && game.history[(game.round - 1) as usize].is_empty()
        {
            return self.eval_multi_policy_leaf_shared(game, ctx, nodes, rng);
        }

        let cur_p = game.current_player();
        let legal = filter_subgame_legal_actions(game, cur_p);
        if legal.is_empty() {
            return game.get_returns()[ctx.updating_player];
        }

        let key_hash = compute_subgame_key_hash(
            game,
            ctx.start_round,
            cur_p,
            ctx.precomputed_buckets,
            ctx.precomputed_ext_river,
        );

        let raw_strategy = if let Some(node) = nodes.get(&key_hash) {
            node.get_strategy()
        } else {
            let node = nodes
                .entry(key_hash)
                .or_insert_with(|| create_prior_infoset_node(game, cur_p, ctx, &legal));
            node.get_strategy()
        };
        let mut strategy = normalize_legal_strategy(&raw_strategy, &legal);
        apply_opponent_node_lock(&mut strategy, game, cur_p, ctx, &legal);

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

        /* Depth-limited Pluribus-style multi-policy continuation leaf evaluation */
        if game.round > ctx.start_round
            && (game.round - ctx.start_round) >= ctx.effective_max_depth
            && game.history[(game.round - 1) as usize].is_empty()
        {
            return self.eval_multi_policy_leaf_local(game, ctx, nodes, rng);
        }

        let cur_p = game.current_player();
        let legal = filter_subgame_legal_actions(game, cur_p);
        if legal.is_empty() {
            return game.get_returns()[ctx.updating_player];
        }

        let key_hash = compute_subgame_key_hash(
            game,
            ctx.start_round,
            cur_p,
            ctx.precomputed_buckets,
            ctx.precomputed_ext_river,
        );

        let raw_strategy = {
            let node = nodes
                .entry(key_hash)
                .or_insert_with(|| create_prior_infoset_node(game, cur_p, ctx, &legal));
            node.get_strategy()
        };
        let mut strategy = normalize_legal_strategy(&raw_strategy, &legal);
        apply_opponent_node_lock(&mut strategy, game, cur_p, ctx, &legal);

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

#[inline]
fn leaf_policy_propensities(b_base: usize, b_ext: usize, round: u8, pol: usize) -> (f64, f64) {
    let (mut bet_p, fold_p): (f64, f64) = if b_base >= BUCKET_TWO_PAIR_MIN {
        match pol {
            POLICY_TIGHT_FOLD => (0.45, 0.10),
            POLICY_STATION_CALL => (0.50, 0.01),
            POLICY_AGGRESSIVE => (0.88, 0.02),
            _ => (0.65, 0.02),
        }
    } else if b_base >= BUCKET_TOP_PAIR_MIN {
        match pol {
            POLICY_TIGHT_FOLD => (0.20, 0.35),
            POLICY_STATION_CALL => (0.22, 0.08),
            POLICY_AGGRESSIVE => (0.62, 0.12),
            _ => (0.38, 0.16),
        }
    } else if round < ROUND_RIVER
        && (BUCKET_STRONG_DRAW_MIN..BUCKET_WEAK_PAIR_MIN).contains(&b_base)
    {
        match pol {
            POLICY_TIGHT_FOLD => (0.12, 0.42),
            POLICY_STATION_CALL => (0.18, 0.10),
            POLICY_AGGRESSIVE => (0.70, 0.14),
            _ => (0.35, 0.20),
        }
    } else if b_base >= BUCKET_WEAK_PAIR_MIN {
        match pol {
            POLICY_TIGHT_FOLD => (0.06, 0.70),
            POLICY_STATION_CALL => (0.08, 0.18),
            POLICY_AGGRESSIVE => (0.32, 0.38),
            _ => (0.14, 0.45),
        }
    } else {
        match pol {
            POLICY_TIGHT_FOLD => (0.05, 0.88),
            POLICY_STATION_CALL => (0.08, 0.55),
            POLICY_AGGRESSIVE => (0.48, 0.62),
            _ => (0.16, 0.72),
        }
    };
    if round >= ROUND_RIVER
        && (BUCKET_RIVER_NUT_FLUSH_BLOCKER_AIR..=BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_AIR)
            .contains(&b_ext)
    {
        bet_p = (bet_p + 0.08).min(0.95);
    }
    (bet_p, fold_p)
}

/** Zero-sum Pluribus-style continuation leaf evaluator across `(p0_pol, p1_pol)` pairs.
 * Captures future betting & fold-equity realization (IP 1.08x vs OOP 0.92x), strong-draw
 * implied odds on Turn (`15..=27`), and extended nut-blocker bluffing propensities. */
#[inline]
pub(super) fn evaluate_continuation_leaf_payoff(
    game: &TexasHoldemGame,
    updating_player: usize,
    precomputed_buckets: [[u8; 2]; 3],
    precomputed_ext_river: [u8; 2],
    p0_pol: usize,
    p1_pol: usize,
) -> f64 {
    let ret0 = evaluate_leaf_rollout(game, 0);
    let max_c = game.contributions[0].max(game.contributions[1]);
    let eff_rem = (game.stack_size - max_c).max(0) as f64;
    if eff_rem <= 1.0 || !(ROUND_FLOP..=ROUND_RIVER).contains(&game.round) {
        return if updating_player == 0 { ret0 } else { -ret0 };
    }

    let c0 = game.contributions[0] as f64;
    let pot = (game.contributions[0] + game.contributions[1]) as f64;
    let street_idx = (game.round - 2) as usize;
    let b0_base = precomputed_buckets[street_idx][0] as usize;
    let b1_base = precomputed_buckets[street_idx][1] as usize;
    let (b0_ext, b1_ext) = if game.round >= ROUND_RIVER {
        (
            precomputed_ext_river[0] as usize,
            precomputed_ext_river[1] as usize,
        )
    } else {
        (b0_base, b1_base)
    };

    let (bet0, fold0) = leaf_policy_propensities(b0_base, b0_ext, game.round, p0_pol);
    let (bet1, fold1) = leaf_policy_propensities(b1_base, b1_ext, game.round, p1_pol);

    let street_mult = if game.round <= ROUND_TURN { 1.25 } else { 1.0 };
    let base_bet_size = (pot * 0.45).min(eff_rem * 0.65);
    let showdown_sign = if ret0 > 0.0 {
        1.0
    } else if ret0 < 0.0 {
        -1.0
    } else {
        0.0
    };

    /* P0 (SB/BTN = IP) bets with position realization multiplier 1.08x */
    let eff_bet0 = bet0 * 1.08 * 0.35 * street_mult;
    let ip_delta =
        eff_bet0 * (fold1 * ((pot - c0) - ret0) + (1.0 - fold1) * showdown_sign * base_bet_size);

    /* P1 (BB = OOP) leads/bets with remaining frequency `(1 - 0.5*bet0)` and OOP 0.92x multiplier */
    let eff_bet1 = bet1 * (1.0 - 0.5 * bet0) * 0.92 * 0.35 * street_mult;
    let oop_delta =
        eff_bet1 * (fold0 * (-c0 - ret0) + (1.0 - fold0) * showdown_sign * base_bet_size);

    /* Turn strong-draw implied odds when hitting vs a made hand */
    let implied = if game.round < ROUND_RIVER {
        let impl_scale = (pot * 0.35).min(eff_rem * 0.45);
        if (BUCKET_STRONG_DRAW_MIN..BUCKET_WEAK_PAIR_MIN).contains(&b0_base)
            && b1_base >= BUCKET_WEAK_PAIR_MIN
            && ret0 > 0.0
        {
            impl_scale * (1.0 - fold1)
        } else if (BUCKET_STRONG_DRAW_MIN..BUCKET_WEAK_PAIR_MIN).contains(&b1_base)
            && b0_base >= BUCKET_WEAK_PAIR_MIN
            && ret0 < 0.0
        {
            -(impl_scale * (1.0 - fold0))
        } else {
            0.0
        }
    } else {
        0.0
    };

    let min_ev = -c0 - eff_rem;
    let max_ev = (pot - c0) + eff_rem;
    let ev_p0 = (ret0 + ip_delta + oop_delta + implied).clamp(min_ev, max_ev);
    if updating_player == 0 { ev_p0 } else { -ev_p0 }
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
