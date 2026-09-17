/*
 * Real-Time Subgame Search (Subgame Solving) for Texas Hold'em.
 * Solves depth-limited subgame online at Turn/River nodes during live play.
 */

use crate::cfr::abstraction::get_holdem_infoset_key;
use crate::cfr::node::InfosetNode;
use crate::game::holdem::{Card, TexasHoldemGame, ALL_52_CARDS};
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::collections::HashMap;

pub struct SubgameSolver {
    pub num_iterations: usize,
}

impl SubgameSolver {
    pub fn new(num_iterations: usize) -> Self {
        SubgameSolver { num_iterations }
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
        let mut nodes: HashMap<String, InfosetNode> = HashMap::new();

        let seed = ((hole[0].rank as u64) << 40)
            ^ ((hole[1].rank as u64) << 32)
            ^ (board.len() as u64 * 0x1234_5678);
        let mut rng = SmallRng::seed_from_u64(seed ^ 0x9E37_79B9_7F4A_7C15);

        let is_used = |c: Card| hole.contains(&c) || board.contains(&c);
        let remaining_deck: Vec<Card> = ALL_52_CARDS
            .iter()
            .copied()
            .filter(|&c| !is_used(c))
            .collect();

        if remaining_deck.len() < 2 {
            return vec![0.0, 1.0, 0.0, 0.0];
        }

        /* Run local CFR+ iterations */
        for iter_idx in 0..self.num_iterations {
            let mut deck = remaining_deck.clone();
            deck.shuffle(&mut rng);
            let opp_hole = [deck[0], deck[1]];

            let (p0_hole, p1_hole) = if my_player == 0 {
                (*hole, opp_hole)
            } else {
                (opp_hole, *hole)
            };

            let game = TexasHoldemGame {
                hole: [p0_hole, p1_hole],
                board: crate::game::holdem::Board::from_slice(board),
                deck_remaining: crate::game::holdem::DeckRemaining::from_slice(&deck[2..]),
                history: [
                    crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(0)),
                    crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(1)),
                    crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(2)),
                    crate::game::holdem::RoundHistory::from_slice(history.actions_in_round(3)),
                ],
                contributions: [100, 100],
                current_player: my_player,
                round,
                raises_this_round: 0,
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
        if let Some(node) = nodes.get(&root_key) {
            node.get_average_strategy()
        } else {
            vec![0.0, 1.0, 0.0, 0.0]
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
                .or_insert_with(|| InfosetNode::new(4));
            node.get_strategy()
        };

        if cur_p == updating_player {
            let mut util = [0.0f64; 4];
            let mut node_util = 0.0f64;

            for &a in &legal {
                let next_game = game.apply_action(a);
                let action_util = self.cfr(&next_game, updating_player, weight, nodes, rng);
                util[a as usize] = action_util;
                node_util += strategy[a as usize] * action_util;
            }

            let mut regrets = vec![0.0f64; 4];
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
