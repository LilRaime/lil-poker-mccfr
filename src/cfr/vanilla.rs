/*
 * Full-tree Vanilla CFR / CFR+ solver for Leduc Poker.
 * Computes exact counterfactual regrets without sampling.
 */

use std::collections::HashMap;
use crate::game::leduc::{LeducGame, NUM_ACTIONS};
use crate::game::card::ALL_CARDS;

pub struct VanillaCFRNode {
    pub regret_sum: Vec<f64>,
    pub strategy_sum: Vec<f64>,
}

impl VanillaCFRNode {
    pub fn new() -> Self {
        VanillaCFRNode {
            regret_sum: vec![0.0; NUM_ACTIONS],
            strategy_sum: vec![0.0; NUM_ACTIONS],
        }
    }

    pub fn get_strategy(&self, _reach_prob: f64) -> Vec<f64> {
        let positive: Vec<f64> = self.regret_sum.iter().map(|&r| r.max(0.0)).collect();
        let sum: f64 = positive.iter().sum();
        let strat = if sum > 0.0 {
            positive.iter().map(|&r| r / sum).collect()
        } else {
            vec![1.0 / NUM_ACTIONS as f64; NUM_ACTIONS]
        };
        strat
    }

    pub fn get_average_strategy(&self) -> Vec<f64> {
        let sum: f64 = self.strategy_sum.iter().sum();
        if sum > 0.0 {
            self.strategy_sum.iter().map(|&s| s / sum).collect()
        } else {
            vec![1.0 / NUM_ACTIONS as f64; NUM_ACTIONS]
        }
    }
}

pub struct VanillaCFRSolver {
    pub nodes: HashMap<String, VanillaCFRNode>,
    pub is_cfr_plus: bool,
}

impl VanillaCFRSolver {
    pub fn new(is_cfr_plus: bool) -> Self {
        VanillaCFRSolver {
            nodes: HashMap::new(),
            is_cfr_plus,
        }
    }

    pub fn train(&mut self, iterations: usize) {
        let cards = &ALL_CARDS;
        let n = cards.len();

        for iter in 1..=iterations {
            let weight = if self.is_cfr_plus { iter as f64 } else { 1.0 };

            /* Traverse all 120 card deals */
            for i in 0..n {
                for j in 0..n {
                    if j == i { continue; }
                    for k in 0..n {
                        if k == i || k == j { continue; }
                        let game = LeducGame::new_with_cards(cards[i], cards[j], cards[k]);
                        self.cfr(&game, [1.0, 1.0], weight);
                    }
                }
            }
        }
    }

    fn cfr(&mut self, game: &LeducGame, reach: [f64; 2], weight: f64) -> f64 {
        if game.is_terminal() {
            /* Return payoff for player 0 (sign flip applied by caller) */
            return game.get_returns()[0];
        }

        let cp = game.current_player();
        let opp = 1 - cp;
        let actions = game.legal_actions();
        let key = game.infoset_key(cp);

        let node = self.nodes.entry(key.clone()).or_insert_with(VanillaCFRNode::new);
        let strategy = node.get_strategy(reach[cp]);

        let legal_probs: Vec<f64> = actions.iter().map(|&a| strategy[a as usize]).collect();
        let prob_sum: f64 = legal_probs.iter().sum();
        let legal_probs: Vec<f64> = if prob_sum > 0.0 {
            legal_probs.iter().map(|p| p / prob_sum).collect()
        } else {
            vec![1.0 / actions.len() as f64; actions.len()]
        };

        /* Accumulate average strategy */
        let node_ref = self.nodes.get_mut(&key).unwrap();
        for (idx, &act) in actions.iter().enumerate() {
            node_ref.strategy_sum[act as usize] += reach[cp] * weight * legal_probs[idx];
        }

        let mut util = 0.0;
        let mut action_utils = vec![0.0; actions.len()];

        for (idx, &act) in actions.iter().enumerate() {
            let mut next_reach = reach;
            next_reach[cp] *= legal_probs[idx];

            let child = game.apply_action(act);
            action_utils[idx] = if child.is_terminal() {
                child.get_returns()[cp]
            } else if child.current_player() == cp {
                self.cfr(&child, next_reach, weight)
            } else {
                -self.cfr(&child, next_reach, weight)
            };

            util += legal_probs[idx] * action_utils[idx];
        }

        /* Update counterfactual regrets */
        let node_ref = self.nodes.get_mut(&key).unwrap();
        for (idx, &act) in actions.iter().enumerate() {
            let regret = reach[opp] * (action_utils[idx] - util);
            node_ref.regret_sum[act as usize] += regret;
            if self.is_cfr_plus && node_ref.regret_sum[act as usize] < 0.0 {
                node_ref.regret_sum[act as usize] = 0.0;
            }
        }

        util
    }

    pub fn export_strategy(&self) -> HashMap<String, Vec<f64>> {
        self.nodes.iter()
            .map(|(k, v)| (k.clone(), v.get_average_strategy()))
            .collect()
    }
}
