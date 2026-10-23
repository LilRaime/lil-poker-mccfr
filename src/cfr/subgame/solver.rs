/*
 * Real-Time Subgame Search (Subgame Solving) for Texas Hold'em.
 * Solves depth-limited subgame online at Turn/River nodes during live play.
 */

use super::reach::OpponentReachDistribution;
use crate::cfr::abstraction::{
    hash_holdem_infoset_key, hash_holdem_infoset_key_with_bucket, postflop_equity_bucket,
};
use crate::cfr::node::InfosetNode;
use crate::game::config::*;
use crate::game::holdem::{Card, TexasHoldemGame, ALL_52_CARDS};
use dashmap::DashMap;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct SubgameSolver {
    pub num_iterations: usize,
    pub adaptive: bool,
    pub threads: usize,
    pub max_depth_streets: u8,
}

/* Reusable context holding allocated InfosetNode maps across subgame solver invocations */
#[derive(Default)]
pub struct SubgameContext {
    pub nodes: HashMap<u64, InfosetNode>,
}

impl SubgameContext {
    pub fn new() -> Self {
        SubgameContext {
            nodes: HashMap::with_capacity(512),
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.nodes.clear();
    }
}

impl Default for SubgameSolver {
    fn default() -> Self {
        Self::new(DEFAULT_SUBGAME_ITERATIONS)
    }
}

impl SubgameSolver {
    pub fn new(num_iterations: usize) -> Self {
        let default_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(4);
        SubgameSolver {
            num_iterations,
            adaptive: true,
            threads: default_threads,
            max_depth_streets: 2,
        }
    }

    pub fn with_adaptive(mut self, adaptive: bool) -> Self {
        self.adaptive = adaptive;
        self
    }

    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    pub fn with_max_depth_streets(mut self, max_depth_streets: u8) -> Self {
        self.max_depth_streets = max_depth_streets.max(1);
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
        self.solve_with_state(
            hole,
            board,
            round,
            history,
            my_player,
            [BIG_BLIND, BIG_BLIND],
            0,
        )
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
        let mut ctx = SubgameContext::new();
        self.solve_with_context(
            hole,
            board,
            round,
            history,
            my_player,
            contributions,
            raises_this_round,
            &mut ctx,
        )
    }

    /* Solves subgame reusing pre-allocated node buffer from SubgameContext. */
    #[allow(clippy::too_many_arguments)]
    pub fn solve_with_context<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        &self,
        hole: &[Card; 2],
        board: &[Card],
        round: u8,
        history: &H,
        my_player: usize,
        contributions: [i32; 2],
        raises_this_round: u8,
        ctx: &mut SubgameContext,
    ) -> Vec<f64> {
        self.solve_with_context_and_strategy(
            hole,
            board,
            round,
            history,
            my_player,
            contributions,
            raises_this_round,
            ctx,
            None,
        )
    }

    /* Solves subgame with explicit blueprint-informed opponent reach ranges */
    #[allow(clippy::too_many_arguments)]
    pub fn solve_with_context_and_strategy<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        &self,
        hole: &[Card; 2],
        board: &[Card],
        round: u8,
        history: &H,
        my_player: usize,
        contributions: [i32; 2],
        raises_this_round: u8,
        ctx: &mut SubgameContext,
        strategy_map: Option<&HashMap<String, Vec<f64>>>,
    ) -> Vec<f64> {
        use rand::Rng;

        ctx.clear();

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
            let mut uniform = vec![0.0; HOLDEM_NUM_ACTIONS];
            uniform[CALL_CHECK as usize] = 1.0;
            return uniform;
        }

        let nodes = &mut ctx.nodes;

        let effective_iterations = self.compute_budget(round, contributions, my_player);

        /* Construct explicit Bayesian reach range distribution across all remaining combos */
        let reach_dist = OpponentReachDistribution::build_detailed(
            &remaining_deck,
            board,
            round,
            history,
            strategy_map,
            Some(my_player),
        );

        /* Construct Hero's own Bayesian reach distribution for Range-vs-Range Continual Resolving */
        let hero_reach_opt = if effective_iterations >= 150 {
            Some(OpponentReachDistribution::build_detailed(
                &remaining_deck,
                board,
                round,
                history,
                strategy_map,
                Some(1 - my_player),
            ))
        } else {
            None
        };

        /* Extract root infoset hash */
        let root_hash = hash_holdem_infoset_key(hole, board, round, history);

        /* Warm-start root node from blueprint strategy if available */
        let blueprint_root_opt = strategy_map.and_then(|smap| {
            crate::cfr::fallback::lookup_strategy_hierarchical_owned(
                |k| smap.get(k).cloned(),
                hole,
                board,
                round,
                history,
            )
        });

        let hist_rounds = [
            crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(0)),
            crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(1)),
            crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(2)),
            crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(3)),
        ];

        let available_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let eff_threads = self.threads.min(available_threads);

        /* Run local CFR+ iterations with O(log K) Bayesian reach sampling (shared table across threads) */
        let solved_strategy: Option<Vec<f64>> = if eff_threads > 1 && effective_iterations >= 200 {
            use rayon::prelude::*;
            let num_threads = eff_threads.min(8);
            let iters_per_thread = effective_iterations.div_ceil(num_threads);
            let shared_nodes: DashMap<u64, InfosetNode> =
                DashMap::with_capacity_and_shard_amount(512, 64);

            if let Some(ref bp_probs) = blueprint_root_opt {
                let root_node = InfosetNode::new(HOLDEM_NUM_ACTIONS);
                root_node.accumulate_strategy(bp_probs, 25.0);
                let init_regrets: Vec<f64> = bp_probs.iter().map(|&p| p * 50.0).collect();
                root_node.update_regrets_cfr_plus(&init_regrets);
                shared_nodes.insert(root_hash, root_node);
            }

            (0..num_threads).into_par_iter().for_each(|t_id| {
                let t_seed = seed
                    .wrapping_add((t_id as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
                    .wrapping_add(0x517c_c1b7_2722_0a95);
                let mut t_rng = SmallRng::seed_from_u64(t_seed);

                for iter_idx in 0..iters_per_thread {
                    let opp_hole = reach_dist.sample(&mut t_rng);
                    let updating_player = (iter_idx + t_id) % 2;

                    /* Range-vs-Range sampling: when opponent is updating_player on every 4th iteration,
                     * sample hero's hand from hero_reach so opponent optimizes against our full range */
                    let hero_hole = if updating_player != my_player && iter_idx % 4 == 1 {
                        if let Some(ref hr) = hero_reach_opt {
                            let cand = hr.sample(&mut t_rng);
                            if !cand.contains(&opp_hole[0]) && !cand.contains(&opp_hole[1]) {
                                cand
                            } else {
                                *hole
                            }
                        } else {
                            *hole
                        }
                    } else {
                        *hole
                    };

                    let (p0_hole, p1_hole) = if my_player == 0 {
                        (hero_hole, opp_hole)
                    } else {
                        (opp_hole, hero_hole)
                    };

                    let mut deck_rem_cards = [ALL_52_CARDS[0]; NUM_CARDS_DECK];
                    let mut rem_count = 0;
                    for &c in &remaining_deck {
                        if c != opp_hole[0]
                            && c != opp_hole[1]
                            && c != hero_hole[0]
                            && c != hero_hole[1]
                        {
                            deck_rem_cards[rem_count] = c;
                            rem_count += 1;
                        }
                    }
                    let num_board_missing = NUM_COMMUNITY_CARDS.saturating_sub(board.len());
                    for i in 0..num_board_missing.min(rem_count) {
                        let j = t_rng.gen_range(i..rem_count);
                        deck_rem_cards.swap(i, j);
                    }

                    let mut full_board_5 = [ALL_52_CARDS[0]; NUM_COMMUNITY_CARDS];
                    for (i, &c) in board.iter().take(NUM_COMMUNITY_CARDS).enumerate() {
                        full_board_5[i] = c;
                    }
                    for i in 0..num_board_missing.min(rem_count) {
                        full_board_5[board.len() + i] = deck_rem_cards[i];
                    }

                    let precomputed_buckets = [
                        [
                            postflop_equity_bucket(&p0_hole, &full_board_5[..3]) as u8,
                            postflop_equity_bucket(&p1_hole, &full_board_5[..3]) as u8,
                        ],
                        [
                            postflop_equity_bucket(&p0_hole, &full_board_5[..4]) as u8,
                            postflop_equity_bucket(&p1_hole, &full_board_5[..4]) as u8,
                        ],
                        [
                            postflop_equity_bucket(&p0_hole, &full_board_5[..5]) as u8,
                            postflop_equity_bucket(&p1_hole, &full_board_5[..5]) as u8,
                        ],
                    ];

                    let game = TexasHoldemGame {
                        hole: [p0_hole, p1_hole],
                        board: crate::game::holdem::Board::from_slice(board),
                        deck_remaining: crate::game::holdem::DeckRemaining::from_slice(
                            &deck_rem_cards[..rem_count],
                        ),
                        history: hist_rounds,
                        contributions,
                        current_player: my_player,
                        round,
                        raises_this_round,
                        terminal: false,
                        returns: [0.0, 0.0],
                    };

                    self.cfr_shared(
                        &game,
                        round,
                        updating_player,
                        (iter_idx + 1) as f64,
                        &precomputed_buckets,
                        &shared_nodes,
                        &mut t_rng,
                    );
                }
            });

            shared_nodes
                .get(&root_hash)
                .map(|n| n.value().get_average_strategy())
        } else {
            if let Some(ref bp_probs) = blueprint_root_opt {
                let root_node = InfosetNode::new(HOLDEM_NUM_ACTIONS);
                root_node.accumulate_strategy(bp_probs, 25.0);
                let init_regrets: Vec<f64> = bp_probs.iter().map(|&p| p * 50.0).collect();
                root_node.update_regrets_cfr_plus(&init_regrets);
                nodes.insert(root_hash, root_node);
            }

            for iter_idx in 0..effective_iterations {
                let opp_hole = reach_dist.sample(&mut rng);
                let updating_player = iter_idx % 2;

                let hero_hole = if updating_player != my_player && iter_idx % 4 == 1 {
                    if let Some(ref hr) = hero_reach_opt {
                        let cand = hr.sample(&mut rng);
                        if !cand.contains(&opp_hole[0]) && !cand.contains(&opp_hole[1]) {
                            cand
                        } else {
                            *hole
                        }
                    } else {
                        *hole
                    }
                } else {
                    *hole
                };

                let (p0_hole, p1_hole) = if my_player == 0 {
                    (hero_hole, opp_hole)
                } else {
                    (opp_hole, hero_hole)
                };

                let mut deck_rem_cards = [ALL_52_CARDS[0]; NUM_CARDS_DECK];
                let mut rem_count = 0;
                for &c in &remaining_deck {
                    if c != opp_hole[0]
                        && c != opp_hole[1]
                        && c != hero_hole[0]
                        && c != hero_hole[1]
                    {
                        deck_rem_cards[rem_count] = c;
                        rem_count += 1;
                    }
                }
                let num_board_missing = NUM_COMMUNITY_CARDS.saturating_sub(board.len());
                for i in 0..num_board_missing.min(rem_count) {
                    let j = rng.gen_range(i..rem_count);
                    deck_rem_cards.swap(i, j);
                }

                let mut full_board_5 = [ALL_52_CARDS[0]; NUM_COMMUNITY_CARDS];
                for (i, &c) in board.iter().take(NUM_COMMUNITY_CARDS).enumerate() {
                    full_board_5[i] = c;
                }
                for i in 0..num_board_missing.min(rem_count) {
                    full_board_5[board.len() + i] = deck_rem_cards[i];
                }

                let precomputed_buckets = [
                    [
                        postflop_equity_bucket(&p0_hole, &full_board_5[..3]) as u8,
                        postflop_equity_bucket(&p1_hole, &full_board_5[..3]) as u8,
                    ],
                    [
                        postflop_equity_bucket(&p0_hole, &full_board_5[..4]) as u8,
                        postflop_equity_bucket(&p1_hole, &full_board_5[..4]) as u8,
                    ],
                    [
                        postflop_equity_bucket(&p0_hole, &full_board_5[..5]) as u8,
                        postflop_equity_bucket(&p1_hole, &full_board_5[..5]) as u8,
                    ],
                ];

                let game = TexasHoldemGame {
                    hole: [p0_hole, p1_hole],
                    board: crate::game::holdem::Board::from_slice(board),
                    deck_remaining: crate::game::holdem::DeckRemaining::from_slice(
                        &deck_rem_cards[..rem_count],
                    ),
                    history: hist_rounds,
                    contributions,
                    current_player: my_player,
                    round,
                    raises_this_round,
                    terminal: false,
                    returns: [0.0, 0.0],
                };

                self.cfr_local(
                    &game,
                    round,
                    updating_player,
                    (iter_idx + 1) as f64,
                    &precomputed_buckets,
                    nodes,
                    &mut rng,
                );
            }
            nodes.get(&root_hash).map(|n| n.get_average_strategy())
        };

        /* Extract average strategy for root infoset */
        let opp = 1 - my_player;
        let to_call = (contributions[opp] - contributions[my_player]).max(0);
        let pot = contributions[0] + contributions[1];
        let legal = if to_call > 0 {
            vec![
                FOLD,
                CALL_CHECK,
                RAISE_MIN,
                RAISE_THIRD_POT,
                RAISE_HALF_POT,
                ALL_IN,
            ]
        } else {
            vec![
                CALL_CHECK,
                RAISE_MIN,
                RAISE_THIRD_POT,
                RAISE_HALF_POT,
                ALL_IN,
            ]
        };
        let fallback = if let Some(ref bp) = blueprint_root_opt {
            let mut arr = [0.0f64; HOLDEM_NUM_ACTIONS];
            for (i, &v) in bp.iter().enumerate().take(HOLDEM_NUM_ACTIONS) {
                arr[i] = v;
            }
            arr
        } else {
            crate::cfr::fallback::get_holdem_fallback_strategy(
                hole, board, round, to_call, pot, &legal,
            )
        };

        if let Some(mut solved) = solved_strategy {
            /* Safe Resolving (Burch et al. 2014):
             * Blend real-time solved subgame policy with robust GTO fallback bounds
             * to guarantee gift-proofing and prevent out-of-distribution bluff spew. */
            let current_bucket = crate::cfr::abstraction::postflop_equity_bucket(hole, board);
            let is_air_or_board_pair =
                current_bucket < 15 || current_bucket == 28 || current_bucket == 29;
            if is_air_or_board_pair {
                solved[ALL_IN as usize] = 0.0;
                if to_call > 0 && (current_bucket < 15 || current_bucket == 28) {
                    for r_act in RAISE_MIN..=ALL_IN {
                        solved[r_act as usize] = 0.0;
                    }
                }
            }
            let (solved_w, fb_w) = if is_air_or_board_pair {
                (0.35, 0.65)
            } else {
                (0.85, 0.15)
            };
            let mut resolved = vec![0.0f64; HOLDEM_NUM_ACTIONS];
            let mut sum = 0.0f64;
            for &a in &legal {
                let idx = a as usize;
                let val = (solved_w * solved[idx] + fb_w * fallback[idx]).max(0.0);
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

    #[allow(clippy::too_many_arguments)]
    fn cfr_shared(
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
    fn cfr_local(
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
fn evaluate_leaf_rollout(game: &TexasHoldemGame, updating_player: usize) -> f64 {
    use crate::game::holdem::evaluate_7cards;
    let mut full_board = game.board;
    let mut deck_rem = game.deck_remaining;
    while full_board.len() < NUM_COMMUNITY_CARDS && !deck_rem.is_empty() {
        full_board.push(deck_rem.deal_one());
    }
    let s0 = evaluate_7cards(&game.hole[0], &full_board);
    let s1 = evaluate_7cards(&game.hole[1], &full_board);
    let pot = game.contributions[0] + game.contributions[1];
    let ret0 = if s0 > s1 {
        (pot - game.contributions[0]) as f64
    } else if s1 > s0 {
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
