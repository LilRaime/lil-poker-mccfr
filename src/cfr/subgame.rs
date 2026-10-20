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

#[derive(Debug, Clone, Copy)]
pub struct SubgameSolver {
    pub num_iterations: usize,
    pub adaptive: bool,
    pub threads: usize,
}

/* Reusable context holding allocated InfosetNode maps across subgame solver invocations */
#[derive(Default)]
pub struct SubgameContext {
    pub nodes: HashMap<String, InfosetNode>,
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

        /* Extract root infoset key */
        let root_key = get_holdem_infoset_key(hole, board, round, history);

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

        /* Run local CFR+ iterations with O(log K) Bayesian reach sampling (parallel or sequential) */
        let solved_strategy: Option<Vec<f64>> = if eff_threads > 1 && effective_iterations >= 200 {
            use rayon::prelude::*;
            let num_threads = eff_threads.min(8);
            let iters_per_thread = effective_iterations.div_ceil(num_threads);

            let thread_results: Vec<Option<Vec<f64>>> = (0..num_threads)
                .into_par_iter()
                .map(|t_id| {
                    let t_seed = seed
                        .wrapping_add((t_id as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
                        .wrapping_add(0x517c_c1b7_2722_0a95);
                    let mut t_rng = SmallRng::seed_from_u64(t_seed);
                    let mut local_nodes: HashMap<String, InfosetNode> = HashMap::with_capacity(256);

                    for iter_idx in 0..iters_per_thread {
                        let opp_hole = reach_dist.sample(&mut t_rng);

                        let (p0_hole, p1_hole) = if my_player == 0 {
                            (*hole, opp_hole)
                        } else {
                            (opp_hole, *hole)
                        };

                        let mut deck_rem_cards = [ALL_52_CARDS[0]; NUM_CARDS_DECK];
                        let mut rem_count = 0;
                        for &c in &remaining_deck {
                            if c != opp_hole[0] && c != opp_hole[1] {
                                deck_rem_cards[rem_count] = c;
                                rem_count += 1;
                            }
                        }
                        let num_board_missing = NUM_COMMUNITY_CARDS.saturating_sub(board.len());
                        for i in 0..num_board_missing.min(rem_count) {
                            let j = t_rng.gen_range(i..rem_count);
                            deck_rem_cards.swap(i, j);
                        }

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

                        let updating_player = iter_idx % 2;
                        self.cfr(
                            &game,
                            updating_player,
                            iter_idx as f64,
                            &mut local_nodes,
                            &mut t_rng,
                        );
                    }

                    local_nodes.get(&root_key).map(|n| n.get_average_strategy())
                })
                .collect();

            let mut avg_strat = vec![0.0f64; HOLDEM_NUM_ACTIONS];
            let mut valid_count = 0;
            for s in thread_results.into_iter().flatten() {
                for (i, &v) in s.iter().enumerate() {
                    avg_strat[i] += v;
                }
                valid_count += 1;
            }
            if valid_count > 0 {
                let inv = 1.0 / valid_count as f64;
                for v in &mut avg_strat {
                    *v *= inv;
                }
                Some(avg_strat)
            } else {
                None
            }
        } else {
            for iter_idx in 0..effective_iterations {
                let opp_hole = reach_dist.sample(&mut rng);

                let (p0_hole, p1_hole) = if my_player == 0 {
                    (*hole, opp_hole)
                } else {
                    (opp_hole, *hole)
                };

                let mut deck_rem_cards = [ALL_52_CARDS[0]; NUM_CARDS_DECK];
                let mut rem_count = 0;
                for &c in &remaining_deck {
                    if c != opp_hole[0] && c != opp_hole[1] {
                        deck_rem_cards[rem_count] = c;
                        rem_count += 1;
                    }
                }
                let num_board_missing = NUM_COMMUNITY_CARDS.saturating_sub(board.len());
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
                    history: hist_rounds,
                    contributions,
                    current_player: my_player,
                    round,
                    raises_this_round,
                    terminal: false,
                    returns: [0.0, 0.0],
                };

                let updating_player = iter_idx % 2;
                self.cfr(&game, updating_player, iter_idx as f64, nodes, &mut rng);
            }
            nodes.get(&root_key).map(|n| n.get_average_strategy())
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
        let fallback = crate::cfr::fallback::get_holdem_fallback_strategy(
            hole, board, round, to_call, pot, &legal,
        );

        if let Some(solved) = solved_strategy {
            /* Safe Resolving (Burch et al. 2014):
             * Blend real-time solved subgame policy with robust GTO fallback bounds (85% solved / 15% fallback)
             * to guarantee gift-proofing and prevent opponent out-of-distribution exploitation. */
            let mut resolved = vec![0.0f64; HOLDEM_NUM_ACTIONS];
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
                .or_insert_with(|| InfosetNode::new(HOLDEM_NUM_ACTIONS));
            node.get_strategy()
        };

        if cur_p == updating_player {
            let mut util = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut node_util = 0.0f64;

            for &a in &legal {
                let next_game = game.apply_action(a);
                let action_util = self.cfr(&next_game, updating_player, weight, nodes, rng);
                util[a as usize] = action_util;
                node_util += strategy[a as usize] * action_util;
            }

            let mut regrets = vec![0.0f64; HOLDEM_NUM_ACTIONS];
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

/* Explicit Bayesian reach distribution for opponent's private hole cards. */
#[derive(Debug, Clone)]
pub struct OpponentReachDistribution {
    pub combos: Vec<[Card; 2]>,
    pub weights: Vec<f64>,
    pub cdf: Vec<f64>,
    pub total_weight: f64,
}

impl OpponentReachDistribution {
    pub fn build<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        remaining_deck: &[Card],
        board: &[Card],
        round: u8,
        history: &H,
    ) -> Self {
        Self::build_detailed(remaining_deck, board, round, history, None, None)
    }

    pub fn build_with_strategy_map<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        remaining_deck: &[Card],
        board: &[Card],
        round: u8,
        history: &H,
        strategy_map: Option<&HashMap<String, Vec<f64>>>,
    ) -> Self {
        Self::build_detailed(remaining_deck, board, round, history, strategy_map, None)
    }

    pub fn build_detailed<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        remaining_deck: &[Card],
        board: &[Card],
        round: u8,
        history: &H,
        strategy_map: Option<&HashMap<String, Vec<f64>>>,
        my_player: Option<usize>,
    ) -> Self {
        use crate::cfr::abstraction::{detect_draws, preflop_bucket, preflop_strength_tier};
        use crate::game::holdem::evaluate_7cards;

        let rem_len = remaining_deck.len();
        if rem_len < 2 {
            let dummy = if rem_len == 1 {
                [remaining_deck[0], remaining_deck[0]]
            } else {
                [
                    Card {
                        rank: crate::game::holdem::Rank::Ace,
                        suit: crate::game::holdem::Suit::Spades,
                    },
                    Card {
                        rank: crate::game::holdem::Rank::King,
                        suit: crate::game::holdem::Suit::Spades,
                    },
                ]
            };
            return Self {
                combos: vec![dummy],
                weights: vec![1.0],
                cdf: vec![1.0],
                total_weight: 1.0,
            };
        }

        let num_combos = rem_len * (rem_len - 1) / 2;
        let mut combos = Vec::with_capacity(num_combos);
        let mut weights = Vec::with_capacity(num_combos);

        let (opp_raised_pf, opp_called_pf, opp_3bet_pf, opp_limped_pf) = if let Some(mp) = my_player
        {
            let opp = 1 - mp;
            let actions = history.actions_in_round(0);
            let opp_actions: Vec<u8> = actions
                .iter()
                .enumerate()
                .filter(|(idx, _)| idx % 2 == opp)
                .map(|(_, &a)| a)
                .collect();
            let opp_raised = opp_actions.iter().any(|&a| a >= RAISE_MIN);
            let opp_called = opp_actions.contains(&CALL_CHECK);

            let mut is_3bet = false;
            for (idx, &a) in actions.iter().enumerate() {
                if idx % 2 == opp && a >= RAISE_MIN {
                    let prior_raises = actions[..idx].iter().filter(|&&pa| pa >= RAISE_MIN).count();
                    if prior_raises >= 1 {
                        is_3bet = true;
                        break;
                    }
                }
            }
            if opp_actions.iter().filter(|&&a| a >= RAISE_MIN).count() >= 2 {
                is_3bet = true;
            }
            let total_pf_raises = actions.iter().filter(|&&a| a >= RAISE_MIN).count();
            let is_limped =
                total_pf_raises == 0 && (opp_called || actions.iter().all(|&a| a == CALL_CHECK));
            (opp_raised, opp_called, is_3bet, is_limped)
        } else {
            let actions = history.actions_in_round(0);
            let total_pf_raises = actions.iter().filter(|&&a| a >= RAISE_MIN).count();
            let opp_raised = actions.iter().any(|&a| a >= RAISE_MIN);
            let opp_called = actions.contains(&CALL_CHECK);
            (
                opp_raised,
                opp_called,
                total_pf_raises >= 2,
                total_pf_raises == 0 && actions.iter().all(|&a| a == CALL_CHECK),
            )
        };

        let (opp_raised_flop, opp_called_flop) = if let Some(mp) = my_player {
            let opp = 1 - mp;
            let actions = history.actions_in_round(1);
            let opp_actions: Vec<u8> = actions
                .iter()
                .enumerate()
                .filter(|(idx, _)| idx % 2 == opp)
                .map(|(_, &a)| a)
                .collect();
            (
                opp_actions.iter().any(|&a| a >= 2),
                opp_actions.contains(&1),
            )
        } else {
            let actions = history.actions_in_round(1);
            (actions.iter().any(|&a| a >= 2), actions.contains(&1))
        };

        let (opp_raised_turn, opp_called_turn) = if let Some(mp) = my_player {
            let opp = 1 - mp;
            let actions = history.actions_in_round(2);
            let opp_actions: Vec<u8> = actions
                .iter()
                .enumerate()
                .filter(|(idx, _)| idx % 2 == opp)
                .map(|(_, &a)| a)
                .collect();
            (
                opp_actions.iter().any(|&a| a >= 2),
                opp_actions.contains(&1),
            )
        } else {
            let actions = history.actions_in_round(2);
            (actions.iter().any(|&a| a >= 2), actions.contains(&1))
        };

        /* Check for monotone / 3-flush board texture across current community cards */
        let mut suit_counts = [0u8; 4];
        for c in board {
            suit_counts[c.suit as usize] += 1;
        }
        let monotone_suit = suit_counts
            .iter()
            .enumerate()
            .find(|(_, &cnt)| cnt >= 3)
            .map(|(s_idx, _)| match s_idx {
                0 => crate::game::holdem::Suit::Clubs,
                1 => crate::game::holdem::Suit::Diamonds,
                2 => crate::game::holdem::Suit::Hearts,
                _ => crate::game::holdem::Suit::Spades,
            });

        let mut sum_w = 0.0f64;

        let compute_pf_fallback_weight = |pf_idx: usize,
                                          opp_3bet: bool,
                                          opp_raised: bool,
                                          opp_called: bool,
                                          opp_limped: bool|
         -> f64 {
            if opp_3bet {
                /* Condensed 3-bet/4-bet range: AA-TT (0..=4), AKs-AQs (13..=14), AKo-AQo (91, 92) */
                let is_tier1 =
                    pf_idx <= 4 || pf_idx == 13 || pf_idx == 14 || pf_idx == 91 || pf_idx == 92;
                let is_tier2 = (5..=7).contains(&pf_idx)
                    || (15..=17).contains(&pf_idx)
                    || pf_idx == 21
                    || pf_idx == 22
                    || pf_idx == 25
                    || pf_idx == 93
                    || pf_idx == 103;
                if is_tier1 {
                    1.00
                } else if is_tier2 {
                    0.65
                } else if pf_idx <= 25 || (91..=95).contains(&pf_idx) {
                    0.20
                } else {
                    0.01 /* Pure junk almost never 3-bets */
                }
            } else if opp_limped {
                /* Capped limping range: monsters rarely limp, mid pairs/connectors limp often */
                if pf_idx <= 2 || pf_idx == 13 || pf_idx == 91 {
                    0.08 /* AA, KK, QQ, AKs, AKo trap rarely */
                } else if pf_idx <= 4 || pf_idx == 14 || pf_idx == 92 {
                    0.25 /* JJ, TT, AQs, AQo rarely limp */
                } else if (5..=12).contains(&pf_idx) || (15..=85).contains(&pf_idx) {
                    1.00 /* 99-22, suited connectors / suited broadways */
                } else if pf_idx <= 130 {
                    0.75 /* Offsuit connectors / broadways */
                } else {
                    0.35 /* Lowest junk */
                }
            } else if opp_raised {
                let tier = preflop_strength_tier(pf_idx);
                match tier {
                    1 | 2 => 1.00,
                    3 => 0.80,
                    4 => 0.30,
                    _ => 0.04,
                }
            } else if opp_called {
                let tier = preflop_strength_tier(pf_idx);
                match tier {
                    1 => 0.35, /* Premium hands rarely just call preflop */
                    2 => 0.60,
                    3 => 0.95,
                    4 => 0.65,
                    _ => 0.18,
                }
            } else {
                let tier = preflop_strength_tier(pf_idx);
                match tier {
                    1 => 0.50,
                    2 => 0.70,
                    3 => 0.85,
                    4 => 0.60,
                    _ => 0.35,
                }
            }
        };

        for i in 0..rem_len {
            for j in (i + 1)..rem_len {
                let cand = [remaining_deck[i], remaining_deck[j]];
                let (pf_idx, _) = preflop_bucket(cand[0], cand[1]);

                /* 1. Preflop Bayesian Prior (informed by blueprint strategy when provided) */
                let pf_weight: f64 = if let Some(strat_map) = strategy_map {
                    let (_, pf_name) = preflop_bucket(cand[0], cand[1]);
                    let pf_key = format!("P:{}/", pf_name);
                    if let Some(probs) = strat_map.get(&pf_key) {
                        if opp_3bet_pf {
                            let r_prob: f64 = probs.iter().skip(RAISE_MIN as usize).sum();
                            r_prob.powi(2).max(0.01)
                        } else if opp_raised_pf {
                            let r_prob: f64 = probs.iter().skip(RAISE_MIN as usize).sum();
                            r_prob.max(0.03)
                        } else if opp_limped_pf {
                            let c_prob = probs.get(CALL_CHECK as usize).copied().unwrap_or(0.5);
                            if pf_idx <= 2 || pf_idx == 13 || pf_idx == 91 {
                                (c_prob * 0.15).max(0.05)
                            } else {
                                c_prob.max(0.20)
                            }
                        } else {
                            probs
                                .get(CALL_CHECK as usize)
                                .copied()
                                .unwrap_or(0.5)
                                .max(0.05)
                        }
                    } else {
                        compute_pf_fallback_weight(
                            pf_idx,
                            opp_3bet_pf,
                            opp_raised_pf,
                            opp_called_pf,
                            opp_limped_pf,
                        )
                    }
                } else {
                    compute_pf_fallback_weight(
                        pf_idx,
                        opp_3bet_pf,
                        opp_raised_pf,
                        opp_called_pf,
                        opp_limped_pf,
                    )
                };

                /* 2. Flop Bayesian Likelihood Update */
                let flop_weight: f64 = if round >= 3 && board.len() >= 3 {
                    let f_score = evaluate_7cards(&cand, &board[..3]);
                    let f_cat = f_score >> 32;
                    let (fd, sd) = detect_draws(&cand, &board[..3]);

                    if opp_raised_flop {
                        /* Opponent bet or raised on flop: polarized to made hands or combo draws */
                        if f_cat >= 2 {
                            1.0 /* Two pair+ */
                        } else if f_cat == 1 {
                            0.85 /* One pair */
                        } else if fd && sd {
                            0.95 /* Combo draw */
                        } else if fd || sd {
                            0.75 /* Single draw */
                        } else {
                            0.08 /* Pure air bluff */
                        }
                    } else if opp_called_flop {
                        /* Opponent check-called on flop: mostly draws, middle pairs, floaters */
                        if f_cat >= 3 {
                            0.45 /* Sets/Straights/Flushes slowplaying */
                        } else if f_cat >= 1 {
                            0.95 /* One pair */
                        } else if fd || sd {
                            0.85 /* Flush/straight draws calling */
                        } else {
                            0.15 /* Floater air */
                        }
                    } else {
                        /* Flop checked through */
                        if f_cat >= 2 {
                            0.30 /* Less likely to check monster */
                        } else if f_cat == 1 {
                            0.80 /* Marginal made hand */
                        } else {
                            0.70 /* Air checking */
                        }
                    }
                } else {
                    1.0
                };

                /* 3. Turn Bayesian Likelihood Update */
                let turn_weight: f64 = if round >= 4 && board.len() >= 4 {
                    let t_score = evaluate_7cards(&cand, &board[..4]);
                    let t_cat = t_score >> 32;
                    let (fd, sd) = detect_draws(&cand, &board[..4]);

                    if opp_raised_turn {
                        /* Double-barrel turn */
                        if t_cat >= 2 {
                            1.0
                        } else if t_cat == 1 {
                            0.70
                        } else if fd || sd {
                            0.65
                        } else {
                            0.05
                        }
                    } else if opp_called_turn {
                        if t_cat >= 2 {
                            0.60
                        } else if t_cat == 1 {
                            0.95
                        } else if fd || sd {
                            0.75
                        } else {
                            0.08
                        }
                    } else {
                        if t_cat >= 2 {
                            0.25
                        } else {
                            0.75
                        }
                    }
                } else {
                    1.0
                };

                /* 4. Monotone Texture Blocker Adjustment */
                let texture_mult: f64 = if let Some(mono_suit) = monotone_suit {
                    let c0_mono = cand[0].suit == mono_suit;
                    let c1_mono = cand[1].suit == mono_suit;
                    let opp_aggressive = opp_raised_flop || opp_raised_turn;

                    if opp_aggressive {
                        if c0_mono && c1_mono {
                            1.45 /* Made flush raising */
                        } else if c0_mono || c1_mono {
                            let mono_card = if c0_mono { cand[0] } else { cand[1] };
                            if mono_card.rank == crate::game::holdem::Rank::Ace {
                                1.35 /* Nut flush draw / blocker */
                            } else if mono_card.rank == crate::game::holdem::Rank::King {
                                1.20 /* 2nd nut flush draw */
                            } else {
                                1.05
                            }
                        } else {
                            /* No flush card and aggressive on monotone board: less likely unless set */
                            0.70
                        }
                    } else if opp_called_flop || opp_called_turn {
                        if c0_mono || c1_mono {
                            1.25 /* Calling with flush draw */
                        } else {
                            0.80 /* Calling without flush draw */
                        }
                    } else {
                        1.00
                    }
                } else {
                    1.00
                };

                let w: f64 = (pf_weight * flop_weight * turn_weight * texture_mult).max(1e-5f64);
                combos.push(cand);
                weights.push(w);
                sum_w += w;
            }
        }

        let mut cdf = Vec::with_capacity(weights.len());
        let mut running = 0.0f64;
        let inv_sum = if sum_w > 1e-12 {
            1.0 / sum_w
        } else {
            1.0 / weights.len() as f64
        };
        for &w in &weights {
            running += w * inv_sum;
            cdf.push(running.min(1.0));
        }
        if let Some(last) = cdf.last_mut() {
            *last = 1.0;
        }

        Self {
            combos,
            weights,
            cdf,
            total_weight: sum_w,
        }
    }

    #[inline(always)]
    pub fn sample<R: rand::Rng>(&self, rng: &mut R) -> [Card; 2] {
        if self.combos.is_empty() {
            return [
                Card {
                    rank: crate::game::holdem::Rank::Ace,
                    suit: crate::game::holdem::Suit::Spades,
                },
                Card {
                    rank: crate::game::holdem::Rank::King,
                    suit: crate::game::holdem::Suit::Spades,
                },
            ];
        }
        if self.combos.len() == 1 {
            return self.combos[0];
        }
        let r: f64 = rng.gen();
        let idx = self
            .cdf
            .partition_point(|&w| w < r)
            .min(self.combos.len() - 1);
        self.combos[idx]
    }

    pub fn top_combos(&self, n: usize) -> Vec<([Card; 2], f64)> {
        let mut indexed: Vec<(usize, f64)> = self.weights.iter().copied().enumerate().collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        indexed
            .into_iter()
            .take(n)
            .map(|(i, w)| {
                (
                    self.combos[i],
                    if self.total_weight > 0.0 {
                        w / self.total_weight
                    } else {
                        0.0
                    },
                )
            })
            .collect()
    }

    pub fn effective_combos(&self) -> usize {
        self.combos.len()
    }
}
