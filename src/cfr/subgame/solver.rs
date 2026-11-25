/*!
 * Real-Time Subgame Search (Subgame Solving) for Texas Hold'em.
 * Solves depth-limited subgame online at Turn/River nodes during live play.
 */

use super::reach::{OpponentReachDistribution, ReachRequest};
use super::traversal::TraversalCtx;
use crate::cfr::abstraction::{
    BUCKET_STRONG_DRAW_MIN, BUCKET_WEAK_PAIR_MIN, hash_holdem_infoset_key,
    hash_holdem_infoset_key_with_bucket, postflop_equity_bucket, postflop_extended_bucket,
};
use crate::cfr::node::InfosetNode;
use crate::game::config::{
    ALL_IN, BIG_BLIND, CALL_CHECK, DEFAULT_SUBGAME_ITERATIONS, FOLD, HOLDEM_NUM_ACTIONS,
    MAX_RAISES_PER_ROUND, NUM_CARDS_DECK, NUM_COMMUNITY_CARDS, NUM_ROUNDS, RAISE_HALF_POT,
    RAISE_MIN, RAISE_THIRD_POT, ROUND_FLOP, ROUND_RIVER, ROUND_TURN, STACK_SIZE,
    SUBGAME_BET_PRESSURE, SUBGAME_POT_LG, SUBGAME_POT_MD, SUBGAME_POT_SM, SUBGAME_POT_XS,
};
use crate::game::holdem::{ALL_52_CARDS, Card, TexasHoldemGame};
use dashmap::DashMap;
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use rayon::prelude::*;
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

/** Sample one iteration's hole cards, board runout, and precomputed street buckets.
 * - On Hero's update (`sample_hero_from_range == false`): 50% exact `*ctx.hole` for dense root
 *   convergence, 50% Bayesian `hero_reach`.
 * - On Opponent's update (`sample_hero_from_range == true`): ALWAYS sample from `hero_reach`
 *   so Opponent never learns a clairvoyant best response to Hero's exact private cards! */
fn build_iteration_state(
    ctx: &SubgameIterContext<'_>,
    sample_hero_from_range: bool,
    rng: &mut SmallRng,
) -> (TexasHoldemGame, [[u8; 2]; 3], [u8; 2]) {
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
        let j = rng.random_range(i..rem_count);
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
    let precomputed_ext_river = [
        postflop_extended_bucket(&p0_hole, &full_board_5[..5]) as u8,
        postflop_extended_bucket(&p1_hole, &full_board_5[..5]) as u8,
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

    (game, precomputed_buckets, precomputed_ext_river)
}

#[derive(Debug, Clone, Copy)]
pub struct SubgameSolver {
    pub num_iterations: usize,
    pub adaptive: bool,
    pub threads: usize,
    pub max_depth_streets: u8,
    pub stack_size: i32,
}

/** Street-level opponent behavioral frequencies for fine-grained `SubgameSolver` node-locking. */
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OppStreetProfile {
    pub flop_fold_to_cbet: Option<f64>,
    pub turn_fold_to_barrel: Option<f64>,
    pub river_fold_to_bet: Option<f64>,
    pub postflop_raise_rate: Option<f64>,
    pub overbet_freq: Option<f64>,
    pub overbet_value_rate: Option<f64>,
}

/** Reusable context holding allocated `InfosetNode` maps and cross-street posterior ranges. */
#[derive(Default)]
pub struct SubgameContext {
    pub nodes: HashMap<u64, InfosetNode>,
    pub last_opp_reach: Option<super::reach::CachedStreetReach>,
    pub last_hero_reach: Option<super::reach::CachedStreetReach>,
}

impl SubgameContext {
    pub fn new() -> Self {
        SubgameContext {
            nodes: HashMap::with_capacity(512),
            last_opp_reach: None,
            last_hero_reach: None,
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    /** Clears both node table and cached posterior ranges when transitioning to a new hand. */
    #[inline]
    pub fn reset_hand(&mut self) {
        self.nodes.clear();
        self.last_opp_reach = None;
        self.last_hero_reach = None;
    }
}

#[derive(Debug, Clone)]
pub struct SubgameRequest<'a> {
    pub hole: [Card; 2],
    pub board: &'a [Card],
    pub round: u8,
    pub my_player: usize,
    pub contributions: [i32; 2],
    pub raises_this_round: u8,
    pub opp_vpip: Option<f64>,
    pub opp_pfr: Option<f64>,
    pub opp_aggression: Option<f64>,
    pub opp_profile: Option<OppStreetProfile>,
}

impl Default for SubgameRequest<'_> {
    fn default() -> Self {
        Self {
            hole: [Card::default(); 2],
            board: &[],
            round: ROUND_TURN,
            my_player: 0,
            contributions: [BIG_BLIND, BIG_BLIND],
            raises_this_round: 0,
            opp_vpip: None,
            opp_pfr: None,
            opp_aggression: None,
            opp_profile: None,
        }
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
            .map_or(1, std::num::NonZero::get)
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

    /** Computes dynamic iteration budget based on pot size, street, and bet pressure */
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
        if round >= ROUND_RIVER {
            mult *= 1.35;
        } else if round == ROUND_TURN {
            mult *= 1.1;
        } else if round == ROUND_FLOP && pot < BIG_BLIND * 8 && to_call == 0 {
            /* Fast SRP flop subgames (4..8 BB): keep live latency compact with depth-1 continuation */
            mult *= 0.90;
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

    /** Solves subgame rooted at the given request state without an external strategy table. */
    pub fn solve<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        &self,
        req: &SubgameRequest<'_>,
        history: &H,
        ctx: Option<&mut SubgameContext>,
    ) -> Vec<f64> {
        self.solve_with_strategy::<H, Vec<f64>>(req, history, None, ctx)
    }

    /** Solves subgame with optional blueprint-informed reach ranges and `AsRef<[f64]>` strategy table. */
    pub fn solve_with_strategy<
        H: crate::cfr::abstraction::HistoryActions + ?Sized,
        V: AsRef<[f64]> + Sync,
    >(
        &self,
        req: &SubgameRequest<'_>,
        history: &H,
        strategy_map: Option<&HashMap<String, V>>,
        ctx: Option<&mut SubgameContext>,
    ) -> Vec<f64> {
        let mut local_ctx;
        let ctx = match ctx {
            Some(c) => c,
            None => {
                local_ctx = SubgameContext::new();
                &mut local_ctx
            }
        };
        ctx.clear();

        let seed = compute_subgame_seed(req.hole, req.board, req.round, req.contributions);
        let mut rng = SmallRng::seed_from_u64(seed);

        let (remaining_deck, board_only_deck) = build_subgame_decks(req.hole, req.board);
        if remaining_deck.len() < 2 {
            let mut uniform = vec![0.0; HOLDEM_NUM_ACTIONS];
            uniform[CALL_CHECK as usize] = 1.0;
            return uniform;
        }

        let effective_iterations = self.compute_budget(req.round, req.contributions, req.my_player);
        let (reach_dist, hero_reach) = build_reach_ranges(
            req,
            history,
            strategy_map,
            &remaining_deck,
            &board_only_deck,
            ctx,
        );
        ctx.last_opp_reach =
            Some(reach_dist.to_cached_reach(req.round, 1 - req.my_player, req.board));
        ctx.last_hero_reach = Some(hero_reach.to_cached_reach(req.round, req.my_player, req.board));

        let root_hash = if req.round == ROUND_RIVER {
            let root_ext = postflop_extended_bucket(&req.hole, req.board) as u8;
            hash_holdem_infoset_key_with_bucket(&req.hole, root_ext, req.round, history)
        } else {
            hash_holdem_infoset_key(&req.hole, req.board, req.round, history)
        };
        let eff_stack_bb = self.stack_size / BIG_BLIND.max(1);
        let blueprint_root_opt = lookup_blueprint_root(
            strategy_map,
            req.hole,
            req.board,
            req.round,
            history,
            eff_stack_bb,
        );

        let hist_rounds = extract_hist_rounds(history);
        let available_threads =
            std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let eff_stack = self
            .stack_size
            .max(req.contributions[0])
            .max(req.contributions[1]);
        let iter_ctx = SubgameIterContext {
            hole: &req.hole,
            board: req.board,
            round: req.round,
            hist_rounds: &hist_rounds,
            contributions: req.contributions,
            raises_this_round: req.raises_this_round,
            my_player: req.my_player,
            stack_size: eff_stack,
            board_only_deck: &board_only_deck,
            reach_dist: &reach_dist,
            hero_reach_opt: Some(&hero_reach),
        };
        let root_pot = req.contributions[0] + req.contributions[1];
        let effective_max_depth = if req.round == ROUND_FLOP && root_pot < BIG_BLIND * 8 {
            self.max_depth_streets.min(1)
        } else {
            self.max_depth_streets
        };
        let run_params = SubgameRunParams {
            root_hash,
            opp_prior_bias: compute_opp_prior_bias(req.opp_vpip, req.opp_pfr),
            opp_profile: req.opp_profile,
            effective_iterations,
            effective_max_depth,
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

        let (to_call, pot, legal) = build_root_legal_actions(
            req.contributions,
            req.my_player,
            eff_stack,
            req.raises_this_round,
        );
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
                    let (game, precomputed_buckets, precomputed_ext_river) =
                        build_iteration_state(iter_ctx, sample_hero_from_range, &mut t_rng);

                    let ctx = TraversalCtx {
                        start_round: iter_ctx.round,
                        effective_max_depth: params.effective_max_depth,
                        root_hash: params.root_hash,
                        my_player: iter_ctx.my_player,
                        opp_prior_bias: params.opp_prior_bias,
                        opp_profile: params.opp_profile,
                        updating_player,
                        weight: (iter_idx + 1) as f64,
                        precomputed_buckets,
                        precomputed_ext_river,
                    };
                    self.cfr_shared(&game, &ctx, &shared_nodes, &mut t_rng);
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
                let (game, precomputed_buckets, precomputed_ext_river) =
                    build_iteration_state(iter_ctx, sample_hero_from_range, rng);

                let ctx = TraversalCtx {
                    start_round: iter_ctx.round,
                    effective_max_depth: params.effective_max_depth,
                    root_hash: params.root_hash,
                    my_player: iter_ctx.my_player,
                    opp_prior_bias: params.opp_prior_bias,
                    opp_profile: params.opp_profile,
                    updating_player,
                    weight: (iter_idx + 1) as f64,
                    precomputed_buckets,
                    precomputed_ext_river,
                };
                self.cfr_local(&game, &ctx, nodes, rng);
            }
            nodes
                .get(&params.root_hash)
                .map(super::super::node::InfosetNode::get_average_strategy)
        }
    }
}

struct SubgameRunParams {
    root_hash: u64,
    opp_prior_bias: Option<(f64, f64)>,
    opp_profile: Option<OppStreetProfile>,
    effective_iterations: usize,
    effective_max_depth: u8,
    eff_threads: usize,
    seed: u64,
}

fn compute_subgame_seed(
    hole: [Card; 2],
    board: &[Card],
    round: u8,
    contributions: [i32; 2],
) -> u64 {
    /* High-entropy board- and chip-sensitive PRNG seed */
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    for c in hole {
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
    hole: [Card; 2],
    board: &[Card],
    to_call: i32,
    legal: &[u8],
) -> Vec<f64> {
    /* Safe Resolving (Burch et al. 2014): blend solved policy with robust GTO fallback bounds */
    let current_bucket = postflop_equity_bucket(&hole, board);
    let is_air_or_board_pair = current_bucket < BUCKET_STRONG_DRAW_MIN
        || current_bucket == BUCKET_WEAK_PAIR_MIN
        || current_bucket == 29;
    if is_air_or_board_pair {
        solved[ALL_IN as usize] = 0.0;
        if to_call > 0
            && (current_bucket < BUCKET_STRONG_DRAW_MIN || current_bucket == BUCKET_WEAK_PAIR_MIN)
        {
            for r_act in RAISE_MIN..=ALL_IN {
                solved[r_act as usize] = 0.0;
            }
        }
    }
    let (solved_w, fb_w) = if is_air_or_board_pair {
        if to_call == 0 {
            (0.65, 0.35)
        } else {
            (0.40, 0.60)
        }
    } else {
        (0.88, 0.12)
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
    hole: [Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
    eff_stack_bb: i32,
) -> Option<Vec<f64>> {
    strategy_map.and_then(|smap| {
        crate::cfr::fallback::lookup_strategy_hierarchical_owned_with_stack(
            |k| smap.get(k).map(|v| v.as_ref().to_vec()),
            &hole,
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
        crate::cfr::fallback::get_holdem_fallback_strategy(&crate::cfr::fallback::FallbackRequest {
            hole: *iter_ctx.hole,
            board: iter_ctx.board,
            round: iter_ctx.round,
            to_call,
            pot,
            legal,
            ..Default::default()
        })
    };

    if let Some(solved) = solved_strategy {
        apply_safe_resolving(
            solved,
            &fallback,
            *iter_ctx.hole,
            iter_ctx.board,
            to_call,
            legal,
        )
    } else {
        fallback.to_vec()
    }
}

fn build_subgame_decks(hole: [Card; 2], board: &[Card]) -> (Vec<Card>, Vec<Card>) {
    let remaining_deck = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|c| !hole.contains(c) && !board.contains(c))
        .collect();
    let board_only_deck = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|c| !board.contains(c))
        .collect();
    (remaining_deck, board_only_deck)
}

fn build_reach_ranges<H, V>(
    req: &SubgameRequest<'_>,
    history: &H,
    strategy_map: Option<&HashMap<String, V>>,
    remaining_deck: &[Card],
    board_only_deck: &[Card],
    ctx: &SubgameContext,
) -> (OpponentReachDistribution, OpponentReachDistribution)
where
    H: crate::cfr::abstraction::HistoryActions + ?Sized,
    V: AsRef<[f64]>,
{
    let reach_dist = OpponentReachDistribution::build_with_prior(
        &ReachRequest {
            remaining_deck,
            board: req.board,
            round: req.round,
            target_player: Some(1 - req.my_player),
            active_player: Some(req.my_player),
            opp_vpip: req.opp_vpip,
            opp_pfr: req.opp_pfr,
            opp_aggression: req.opp_aggression,
        },
        history,
        strategy_map,
        ctx.last_opp_reach.as_ref(),
    );
    let hero_reach = OpponentReachDistribution::build_with_prior(
        &ReachRequest {
            remaining_deck: board_only_deck,
            board: req.board,
            round: req.round,
            target_player: Some(req.my_player),
            active_player: Some(req.my_player),
            opp_vpip: None,
            opp_pfr: None,
            opp_aggression: None,
        },
        history,
        strategy_map,
        ctx.last_hero_reach.as_ref(),
    );
    (reach_dist, hero_reach)
}

fn extract_hist_rounds<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
    history: &H,
) -> [crate::game::holdem::RoundHistory; NUM_ROUNDS] {
    [
        crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(0)),
        crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(1)),
        crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(2)),
        crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(3)),
    ]
}
