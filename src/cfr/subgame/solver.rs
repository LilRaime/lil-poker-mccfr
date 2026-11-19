/*
 * Real-Time Subgame Search (Subgame Solving) for Texas Hold'em.
 * Solves depth-limited subgame online at Turn/River nodes during live play.
 */

use super::reach::OpponentReachDistribution;
use crate::cfr::abstraction::{hash_holdem_infoset_key, postflop_equity_bucket};
use crate::cfr::node::InfosetNode;
use crate::game::config::*;
use crate::game::holdem::{Card, TexasHoldemGame, ALL_52_CARDS};
use dashmap::DashMap;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use std::collections::HashMap;

struct SubgameIterContext<'a> {
    hole: &'a [Card; 2],
    board: &'a [Card],
    round: u8,
    hist_rounds: &'a [crate::game::holdem::RoundHistory; NUM_ROUNDS],
    contributions: [i32; 2],
    raises_this_round: u8,
    my_player: usize,
    stack_size: i32,
    board_only_deck: &'a [Card],
    reach_dist: &'a OpponentReachDistribution,
    hero_reach_opt: Option<&'a OpponentReachDistribution>,
}

/* Sample one iteration's hole cards, board runout, and precomputed street buckets.
 * - On Hero's update (`sample_hero_from_range == false`): 50% exact `*ctx.hole` for dense root
 *   convergence, 50% Bayesian `hero_reach`.
 * - On Opponent's update (`sample_hero_from_range == true`): ALWAYS sample from `hero_reach`
 *   so Opponent never learns a clairvoyant best response to Hero's exact private cards! */
fn build_iteration_state(
    ctx: &SubgameIterContext<'_>,
    sample_hero_from_range: bool,
    rng: &mut SmallRng,
) -> (TexasHoldemGame, [[u8; 2]; 3]) {
    use rand::Rng;

    let opp_hole = ctx.reach_dist.sample(rng);
    let hero_hole = if sample_hero_from_range {
        if let Some(hr) = ctx.hero_reach_opt {
            let mut chosen = *ctx.hole;
            for _ in 0..8 {
                let cand = hr.sample(rng);
                if !cand.contains(&opp_hole[0]) && !cand.contains(&opp_hole[1]) {
                    chosen = cand;
                    break;
                }
            }
            chosen
        } else {
            *ctx.hole
        }
    } else {
        *ctx.hole
    };

    let (p0_hole, p1_hole) = if ctx.my_player == 0 {
        (hero_hole, opp_hole)
    } else {
        (opp_hole, hero_hole)
    };

    let mut deck_rem_cards = [ALL_52_CARDS[0]; NUM_CARDS_DECK];
    let mut rem_count = 0;
    for &c in ctx.board_only_deck {
        if c != opp_hole[0] && c != opp_hole[1] && c != hero_hole[0] && c != hero_hole[1] {
            deck_rem_cards[rem_count] = c;
            rem_count += 1;
        }
    }
    let num_board_missing = NUM_COMMUNITY_CARDS.saturating_sub(ctx.board.len());
    for i in 0..num_board_missing.min(rem_count) {
        let j = rng.gen_range(i..rem_count);
        deck_rem_cards.swap(i, j);
    }

    let mut full_board_5 = [ALL_52_CARDS[0]; NUM_COMMUNITY_CARDS];
    for (i, &c) in ctx.board.iter().take(NUM_COMMUNITY_CARDS).enumerate() {
        full_board_5[i] = c;
    }
    for i in 0..num_board_missing.min(rem_count) {
        full_board_5[ctx.board.len() + i] = deck_rem_cards[i];
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

    let s0 = crate::game::holdem::evaluate_7cards(&p0_hole, &full_board_5);
    let s1 = crate::game::holdem::evaluate_7cards(&p1_hole, &full_board_5);
    let showdown_outcome = if s0 > s1 {
        1
    } else if s1 > s0 {
        -1
    } else {
        0
    };
    let is_wet_flop = full_board_5[0].suit == full_board_5[1].suit
        || full_board_5[1].suit == full_board_5[2].suit
        || full_board_5[0].suit == full_board_5[2].suit;

    let game = TexasHoldemGame {
        hole: [p0_hole, p1_hole],
        board: crate::game::holdem::Board::from_slice(ctx.board),
        deck_remaining: crate::game::holdem::DeckRemaining::from_slice(
            &deck_rem_cards[..rem_count],
        ),
        history: *ctx.hist_rounds,
        contributions: ctx.contributions,
        current_player: ctx.my_player,
        round: ctx.round,
        raises_this_round: ctx.raises_this_round,
        stack_size: ctx.stack_size,
        terminal: false,
        returns: [0.0, 0.0],
        showdown_outcome,
        is_wet_flop,
    };

    (game, precomputed_buckets)
}

#[derive(Debug, Clone, Copy)]
pub struct SubgameSolver {
    pub num_iterations: usize,
    pub adaptive: bool,
    pub threads: usize,
    pub max_depth_streets: u8,
    pub stack_size: i32,
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
            stack_size: STACK_SIZE,
        }
    }

    pub fn with_stack_size(mut self, stack_size: i32) -> Self {
        self.stack_size = stack_size.max(BIG_BLIND * 2);
        self
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
        self.solve_with_stats(
            hole,
            board,
            round,
            history,
            my_player,
            contributions,
            raises_this_round,
            ctx,
            strategy_map,
            None,
            None,
        )
    }

    /* Solves subgame with explicit blueprint-informed and VPIP/PFR-adapted opponent reach ranges */
    #[allow(clippy::too_many_arguments)]
    pub fn solve_with_stats<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
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
        opp_vpip: Option<f64>,
        opp_pfr: Option<f64>,
    ) -> Vec<f64> {
        self.solve_with_stats_generic(
            hole,
            board,
            round,
            history,
            my_player,
            contributions,
            raises_this_round,
            ctx,
            strategy_map,
            opp_vpip,
            opp_pfr,
        )
    }

    /* Generic subgame solver accepting any `AsRef<[f64]>` strategy table (e.g. `Vec<f64>` or `[f64; N]`) */
    #[allow(clippy::too_many_arguments)]
    pub fn solve_with_stats_generic<
        H: crate::cfr::abstraction::HistoryActions + ?Sized,
        V: AsRef<[f64]> + Sync,
    >(
        &self,
        hole: &[Card; 2],
        board: &[Card],
        round: u8,
        history: &H,
        my_player: usize,
        contributions: [i32; 2],
        raises_this_round: u8,
        ctx: &mut SubgameContext,
        strategy_map: Option<&HashMap<String, V>>,
        opp_vpip: Option<f64>,
        opp_pfr: Option<f64>,
    ) -> Vec<f64> {
        ctx.clear();

        let seed = compute_subgame_seed(hole, board, round, contributions);
        let mut rng = SmallRng::seed_from_u64(seed);

        let is_used = |c: Card| hole.contains(&c) || board.contains(&c);
        let remaining_deck: Vec<Card> = ALL_52_CARDS
            .iter()
            .copied()
            .filter(|&c| !is_used(c))
            .collect();
        let board_only_deck: Vec<Card> = ALL_52_CARDS
            .iter()
            .copied()
            .filter(|&c| !board.contains(&c))
            .collect();

        if remaining_deck.len() < 2 {
            let mut uniform = vec![0.0; HOLDEM_NUM_ACTIONS];
            uniform[CALL_CHECK as usize] = 1.0;
            return uniform;
        }

        let effective_iterations = self.compute_budget(round, contributions, my_player);
        let reach_dist = OpponentReachDistribution::build_for_target_generic(
            &remaining_deck,
            board,
            round,
            history,
            strategy_map,
            Some(1 - my_player),
            Some(my_player),
            opp_vpip,
            opp_pfr,
        );
        let hero_reach_opt = Some(OpponentReachDistribution::build_for_target_generic(
            &board_only_deck,
            board,
            round,
            history,
            strategy_map,
            Some(my_player),
            Some(my_player),
            None,
            None,
        ));

        let root_hash = hash_holdem_infoset_key(hole, board, round, history);
        let eff_stack_bb = self.stack_size / BIG_BLIND.max(1);
        let blueprint_root_opt =
            lookup_blueprint_root(strategy_map, hole, board, round, history, eff_stack_bb);

        let hist_rounds = [
            crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(0)),
            crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(1)),
            crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(2)),
            crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(3)),
        ];
        let available_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let eff_stack = self.stack_size.max(contributions[0]).max(contributions[1]);
        let iter_ctx = SubgameIterContext {
            hole,
            board,
            round,
            hist_rounds: &hist_rounds,
            contributions,
            raises_this_round,
            my_player,
            stack_size: eff_stack,
            board_only_deck: &board_only_deck,
            reach_dist: &reach_dist,
            hero_reach_opt: hero_reach_opt.as_ref(),
        };
        let run_params = SubgameRunParams {
            root_hash,
            opp_prior_bias: compute_opp_prior_bias(opp_vpip, opp_pfr),
            effective_iterations,
            eff_threads: self.threads.min(available_threads),
            seed,
        };
        let solved_strategy = self.run_cfr_iterations(
            &iter_ctx,
            &run_params,
            blueprint_root_opt.as_deref(),
            &mut ctx.nodes,
            &mut rng,
        );

        let (to_call, pot, legal) =
            build_root_legal_actions(contributions, my_player, eff_stack, raises_this_round);
        finalize_root_strategy(
            solved_strategy,
            blueprint_root_opt.as_deref(),
            &iter_ctx,
            to_call,
            pot,
            &legal,
        )
    }

    fn run_cfr_iterations(
        &self,
        iter_ctx: &SubgameIterContext<'_>,
        params: &SubgameRunParams,
        blueprint_root_opt: Option<&[f64]>,
        nodes: &mut HashMap<u64, InfosetNode>,
        rng: &mut SmallRng,
    ) -> Option<Vec<f64>> {
        /* Run local CFR+ iterations:
         * - When updating Opponent: always sample Hero from hero_reach (no hole-card leak!).
         * - When updating Hero: alternate exact *hole (dense root convergence) and hero_reach. */
        if params.eff_threads > 1 && params.effective_iterations >= 200 {
            use rayon::prelude::*;
            let num_threads = params.eff_threads.min(8);
            let iters_per_thread = params.effective_iterations.div_ceil(num_threads);
            let shared_nodes: DashMap<u64, InfosetNode> =
                DashMap::with_capacity_and_shard_amount(512, 64);

            if let Some(bp_probs) = blueprint_root_opt {
                shared_nodes.insert(params.root_hash, make_warm_root(bp_probs));
            }

            (0..num_threads).into_par_iter().for_each(|t_id| {
                let t_seed = params
                    .seed
                    .wrapping_add((t_id as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
                    .wrapping_add(0x517c_c1b7_2722_0a95);
                let mut t_rng = SmallRng::seed_from_u64(t_seed);

                for iter_idx in 0..iters_per_thread {
                    let updating_player = (iter_idx + t_id) % 2;
                    let sample_hero_from_range =
                        updating_player != iter_ctx.my_player || ((iter_idx / 2) % 2) == 1;
                    let (game, precomputed_buckets) =
                        build_iteration_state(iter_ctx, sample_hero_from_range, &mut t_rng);

                    self.cfr_shared(
                        &game,
                        iter_ctx.round,
                        params.root_hash,
                        iter_ctx.my_player,
                        params.opp_prior_bias,
                        updating_player,
                        (iter_idx + 1) as f64,
                        &precomputed_buckets,
                        &shared_nodes,
                        &mut t_rng,
                    );
                }
            });

            shared_nodes
                .get(&params.root_hash)
                .map(|n| n.value().get_average_strategy())
        } else {
            if let Some(bp_probs) = blueprint_root_opt {
                nodes.insert(params.root_hash, make_warm_root(bp_probs));
            }

            for iter_idx in 0..params.effective_iterations {
                let updating_player = iter_idx % 2;
                let sample_hero_from_range =
                    updating_player != iter_ctx.my_player || ((iter_idx / 2) % 2) == 1;
                let (game, precomputed_buckets) =
                    build_iteration_state(iter_ctx, sample_hero_from_range, rng);

                self.cfr_local(
                    &game,
                    iter_ctx.round,
                    params.root_hash,
                    iter_ctx.my_player,
                    params.opp_prior_bias,
                    updating_player,
                    (iter_idx + 1) as f64,
                    &precomputed_buckets,
                    nodes,
                    rng,
                );
            }
            nodes
                .get(&params.root_hash)
                .map(|n| n.get_average_strategy())
        }
    }
}

struct SubgameRunParams {
    root_hash: u64,
    opp_prior_bias: Option<(f64, f64)>,
    effective_iterations: usize,
    eff_threads: usize,
    seed: u64,
}

fn compute_subgame_seed(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    contributions: [i32; 2],
) -> u64 {
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
    seed ^ (round as u64).wrapping_shl(32)
}

fn compute_opp_prior_bias(opp_vpip: Option<f64>, opp_pfr: Option<f64>) -> Option<(f64, f64)> {
    /* Lightweight node-locking bias (fold_mult, raise_mult) for Opponent child priors */
    match (opp_vpip, opp_pfr) {
        (Some(_v), Some(p)) if p > 0.45 => Some((0.75, 1.45)), /* Maniac: folds less, raises more */
        (Some(v), Some(p)) if v > 0.68 && p < 0.22 => Some((0.65, 0.70)), /* Station: calls more */
        (Some(v), _) if v < 0.25 => Some((1.40, 0.85)),        /* Nit/Rock: folds more */
        _ => None,
    }
}

fn make_warm_root(bp_probs: &[f64]) -> InfosetNode {
    let root_node = InfosetNode::new(HOLDEM_NUM_ACTIONS);
    root_node.accumulate_strategy(bp_probs, 25.0);
    let init_regrets: Vec<f64> = bp_probs.iter().map(|&p| p * 50.0).collect();
    root_node.update_regrets_cfr_plus(&init_regrets);
    root_node
}

fn build_root_legal_actions(
    contributions: [i32; 2],
    my_player: usize,
    eff_stack: i32,
    raises_this_round: u8,
) -> (i32, i32, Vec<u8>) {
    let opp = 1 - my_player;
    let to_call = (contributions[opp] - contributions[my_player]).max(0);
    let pot = contributions[0] + contributions[1];
    let any_allin = contributions[opp] >= eff_stack || contributions[my_player] >= eff_stack;
    let mut legal = if any_allin {
        if to_call > 0 {
            vec![FOLD, CALL_CHECK]
        } else {
            vec![CALL_CHECK]
        }
    } else if raises_this_round >= MAX_RAISES_PER_ROUND {
        if to_call > 0 {
            vec![FOLD, CALL_CHECK, ALL_IN]
        } else {
            vec![CALL_CHECK, ALL_IN]
        }
    } else if to_call > 0 {
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
    if legal.contains(&ALL_IN) && legal.iter().any(|&a| (RAISE_MIN..ALL_IN).contains(&a)) {
        let rem_stack = (eff_stack - contributions[my_player]).max(0);
        let rem_after_call = (rem_stack - to_call).max(0);
        let pot_after_call = pot.max(1) + to_call;
        if rem_after_call > pot_after_call * 3 {
            legal.retain(|&a| a != ALL_IN);
        }
    }
    (to_call, pot, legal)
}

fn apply_safe_resolving(
    mut solved: Vec<f64>,
    fallback: &[f64; HOLDEM_NUM_ACTIONS],
    hole: &[Card; 2],
    board: &[Card],
    to_call: i32,
    legal: &[u8],
) -> Vec<f64> {
    /* Safe Resolving (Burch et al. 2014): blend solved policy with robust GTO fallback bounds */
    let current_bucket = postflop_equity_bucket(hole, board);
    let is_air_or_board_pair = current_bucket < 15 || current_bucket == 28 || current_bucket == 29;
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
    for &a in legal {
        let idx = a as usize;
        let val = (solved_w * solved[idx] + fb_w * fallback[idx]).max(0.0);
        resolved[idx] = val;
        sum += val;
    }
    if sum > 1e-9 {
        let inv = 1.0 / sum;
        for &a in legal {
            resolved[a as usize] *= inv;
        }
    }
    resolved
}

fn lookup_blueprint_root<H: crate::cfr::abstraction::HistoryActions + ?Sized, V: AsRef<[f64]>>(
    strategy_map: Option<&HashMap<String, V>>,
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
    eff_stack_bb: i32,
) -> Option<Vec<f64>> {
    strategy_map.and_then(|smap| {
        crate::cfr::fallback::lookup_strategy_hierarchical_owned_with_stack(
            |k| smap.get(k).map(|v| v.as_ref().to_vec()),
            hole,
            board,
            round,
            history,
            eff_stack_bb,
        )
    })
}

fn finalize_root_strategy(
    solved_strategy: Option<Vec<f64>>,
    blueprint_root_opt: Option<&[f64]>,
    iter_ctx: &SubgameIterContext<'_>,
    to_call: i32,
    pot: i32,
    legal: &[u8],
) -> Vec<f64> {
    let fallback = if let Some(bp) = blueprint_root_opt {
        let mut arr = [0.0f64; HOLDEM_NUM_ACTIONS];
        for (i, &v) in bp.iter().enumerate().take(HOLDEM_NUM_ACTIONS) {
            arr[i] = v;
        }
        arr
    } else {
        crate::cfr::fallback::get_holdem_fallback_strategy(
            iter_ctx.hole,
            iter_ctx.board,
            iter_ctx.round,
            to_call,
            pot,
            legal,
        )
    };

    if let Some(solved) = solved_strategy {
        apply_safe_resolving(
            solved,
            &fallback,
            iter_ctx.hole,
            iter_ctx.board,
            to_call,
            legal,
        )
    } else {
        fallback.to_vec()
    }
}
