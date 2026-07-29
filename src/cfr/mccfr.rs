/* Parallel External Sampling MCCFR with CFR+ for Leduc Poker. */
use dashmap::DashMap;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use crate::cfr::node::InfosetNode;
use crate::game::leduc::{LeducGame, NUM_ACTIONS};

pub struct MCCFRSolver {
    pub nodes: Arc<DashMap<String, Arc<InfosetNode>>>,
}

impl MCCFRSolver {
    pub fn new() -> Self {
        MCCFRSolver {
            nodes: Arc::new(DashMap::new()),
        }
    }

    /* Get or create a node for key. */
    fn get_node(&self, key: &str) -> Arc<InfosetNode> {
        if let Some(node) = self.nodes.get(key) {
            return Arc::clone(&node);
        }
        let node = Arc::new(InfosetNode::new(NUM_ACTIONS));
        self.nodes
            .entry(key.to_string())
            .or_insert_with(|| Arc::clone(&node));
        Arc::clone(&self.nodes.get(key).unwrap())
    }

    /* Run iterations of parallel External Sampling MCCFR. */
    pub fn train(&self, iterations: u64, threads: usize, log_every: u64) {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .unwrap_or(());

        let nodes = Arc::clone(&self.nodes);
        let chunk_size = if log_every > 0 { log_every } else { iterations };
        let width = iterations.to_string().len();
        let mut done = 0u64;

        let entropy_base = Arc::new(AtomicU64::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xcafe_babe_dead_beef),
        ));

        while done < iterations {
            let batch_end = (done + chunk_size).min(iterations);
            let batch = done..batch_end;
            let nodes_ref = Arc::clone(&nodes);
            let entropy_ref = Arc::clone(&entropy_base);

            batch.into_par_iter().for_each(|iter_idx| {
                let thread_entropy = entropy_ref.fetch_add(1, AtomicOrdering::Relaxed);
                let seed = iter_idx.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ thread_entropy;
                let mut rng = SmallRng::seed_from_u64(seed);
                let updating_player = (iter_idx % 2) as usize;
                let game = LeducGame::new_random(&mut rng);
                let solver_ref = MCCFRSolver {
                    nodes: Arc::clone(&nodes_ref),
                };
                solver_ref.traverse(&game, updating_player, iter_idx as f64, &mut rng);
            });

            done = batch_end;

            if log_every > 0 {
                eprint!(
                    "\r  [{:>width$}/{}] nodes={}",
                    done,
                    iterations,
                    nodes.len(),
                    width = width,
                );
            }
        }

        eprintln!();
    }

    fn traverse(
        &self,
        game: &LeducGame,
        updating_player: usize,
        iter_idx: f64,
        rng: &mut SmallRng,
    ) -> f64 {
        if game.is_terminal() {
            return game.get_returns()[updating_player];
        }

        let curr_player = game.current_player();
        let actions = game.legal_actions();
        let n = actions.len();
        if n == 0 {
            return 0.0;
        }

        let key = game.infoset_key(curr_player);
        let node = self.get_node(&key);
        let strategy = node.get_strategy();

        let legal_probs: Vec<f64> = actions.iter().map(|&a| strategy[a as usize]).collect();
        let prob_sum: f64 = legal_probs.iter().sum();
        let legal_probs: Vec<f64> = if prob_sum > 0.0 {
            legal_probs.iter().map(|p| p / prob_sum).collect()
        } else {
            vec![1.0 / n as f64; n]
        };

        if curr_player == updating_player {
            let mut action_utils = vec![0.0f64; n];
            let mut node_util = 0.0f64;

            for (idx, &act) in actions.iter().enumerate() {
                let child = game.apply_action(act);
                action_utils[idx] = self.traverse(&child, updating_player, iter_idx, rng);
                node_util += legal_probs[idx] * action_utils[idx];
            }

            let mut regrets = vec![0.0f64; NUM_ACTIONS];
            for (idx, &act) in actions.iter().enumerate() {
                regrets[act as usize] = action_utils[idx] - node_util;
            }

            node.update_regrets_cfr_plus(&regrets);
            node_util
        } else {
            let chosen_idx = sample_action(&legal_probs, rng);
            let chosen_act = actions[chosen_idx];

            let mut full_strategy = vec![0.0f64; NUM_ACTIONS];
            for (idx, &act) in actions.iter().enumerate() {
                full_strategy[act as usize] = legal_probs[idx];
            }
            node.accumulate_strategy(&full_strategy, 1.0);

            let child = game.apply_action(chosen_act);
            self.traverse(&child, updating_player, iter_idx, rng)
        }
    }

    /* Collect average strategies as a map from infoset key → probabilities */
    pub fn export_strategy(&self) -> std::collections::HashMap<String, Vec<f64>> {
        self.nodes
            .iter()
            .map(|entry| {
                let key = entry.key().clone();
                let avg = entry.value().get_average_strategy();
                (key, avg)
            })
            .collect()
    }
}

/* Sample an index from a probability distribution. */
fn sample_action(probs: &[f64], rng: &mut SmallRng) -> usize {
    let r: f64 = rng.gen();
    let mut cumulative = 0.0;
    for (i, &p) in probs.iter().enumerate() {
        cumulative += p;
        if r < cumulative {
            return i;
        }
    }
    probs.len() - 1
}
