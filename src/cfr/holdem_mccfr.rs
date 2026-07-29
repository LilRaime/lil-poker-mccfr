/* Parallel External Sampling MCCFR Solver for 52-Card Texas Hold'em with Card Abstraction. */

use dashmap::DashMap;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use crate::cfr::abstraction::get_holdem_infoset_key;
use crate::cfr::node::InfosetNode;
use crate::game::holdem::TexasHoldemGame;

pub const HOLDEM_NUM_ACTIONS: usize = 4;

pub struct HoldemMCCFRSolver {
    pub nodes: Arc<DashMap<String, Arc<InfosetNode>>>,
}

impl HoldemMCCFRSolver {
    pub fn new() -> Self {
        HoldemMCCFRSolver {
            nodes: Arc::new(DashMap::new()),
        }
    }

    /* Get or create node for abstracted infoset key */
    fn get_node(&self, key: &str) -> Arc<InfosetNode> {
        if let Some(node) = self.nodes.get(key) {
            return Arc::clone(&node);
        }
        let node = Arc::new(InfosetNode::new(HOLDEM_NUM_ACTIONS));
        self.nodes
            .entry(key.to_string())
            .or_insert_with(|| Arc::clone(&node));
        Arc::clone(&self.nodes.get(key).unwrap())
    }

    /* Run iterations of parallel Bucketed External Sampling MCCFR */
    pub fn train(&self, iterations: u64, threads: usize, log_every: u64) {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .unwrap_or(());

        let nodes = Arc::clone(&self.nodes);
        let chunk_size = if log_every > 0 { log_every } else { iterations };
        let width = iterations.to_string().len();
        let mut done = 0u64;
        let train_start = std::time::Instant::now();

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

            let batch_start = std::time::Instant::now();

            batch.into_par_iter().for_each(|iter_idx| {
                let thread_entropy = entropy_ref.fetch_add(1, AtomicOrdering::Relaxed);
                let seed = iter_idx.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ thread_entropy;
                let mut rng = SmallRng::seed_from_u64(seed);
                let updating_player = (iter_idx % 2) as usize;
                let game = TexasHoldemGame::new_random(&mut rng);
                let solver_ref = HoldemMCCFRSolver {
                    nodes: Arc::clone(&nodes_ref),
                };
                solver_ref.traverse(&game, updating_player, iter_idx as f64, &mut rng);
            });

            done = batch_end;

            if log_every > 0 {
                let elapsed = train_start.elapsed().as_secs_f64();
                let batch_secs = batch_start.elapsed().as_secs_f64().max(1e-9);
                let iter_per_s = chunk_size as f64 / batch_secs;
                let remaining = iterations - done;
                let eta_secs = remaining as f64 / iter_per_s;
                let pct = done as f64 / iterations as f64 * 100.0;

                let fmt_secs = |s: f64| -> String {
                    let s = s as u64;
                    if s < 60 {
                        format!("{s}s")
                    } else if s < 3600 {
                        format!("{}m{:02}s", s / 60, s % 60)
                    } else {
                        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
                    }
                };

                eprintln!(
                    "  [{:>width$}/{} | {:5.1}%]  infosets={:>7}  speed={:.0}k/s  elapsed={}  eta={}",
                    done, iterations, pct,
                    nodes.len(),
                    iter_per_s / 1000.0,
                    fmt_secs(elapsed),
                    fmt_secs(eta_secs),
                    width = width,
                );
            }
        }

        eprintln!();
    }

    fn traverse(
        &self,
        game: &TexasHoldemGame,
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

        let key = get_holdem_infoset_key(
            &game.hole[curr_player],
            &game.board,
            game.round,
            &game.history,
        );
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

            let mut regrets = vec![0.0f64; HOLDEM_NUM_ACTIONS];
            for (idx, &act) in actions.iter().enumerate() {
                regrets[act as usize] = action_utils[idx] - node_util;
            }

            node.update_regrets_cfr_plus(&regrets);
            node_util
        } else {
            let chosen_idx = sample_action(&legal_probs, rng);
            let chosen_act = actions[chosen_idx];

            let mut full_strategy = vec![0.0f64; HOLDEM_NUM_ACTIONS];
            for (idx, &act) in actions.iter().enumerate() {
                full_strategy[act as usize] = legal_probs[idx];
            }
            node.accumulate_strategy(&full_strategy, 1.0);

            let child = game.apply_action(chosen_act);
            self.traverse(&child, updating_player, iter_idx, rng)
        }
    }

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
