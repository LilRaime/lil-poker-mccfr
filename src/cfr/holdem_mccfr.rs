/* Parallel External Sampling MCCFR Solver for 52-Card Texas Hold'em with Card Abstraction. */

use dashmap::DashMap;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use crate::cfr::abstraction::format_holdem_infoset_key;
use crate::cfr::node::InfosetNode;
use crate::game::holdem::TexasHoldemGame;

pub const HOLDEM_NUM_ACTIONS: usize = 4;

pub struct HoldemMCCFRSolver {
    pub nodes: Arc<DashMap<String, Arc<InfosetNode>>>,
}

impl Default for HoldemMCCFRSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl HoldemMCCFRSolver {
    pub fn new() -> Self {
        HoldemMCCFRSolver {
            nodes: Arc::new(DashMap::with_capacity_and_shard_amount(500_000, 1024)),
        }
    }

    /* Fast get or create node for abstracted infoset key */
    #[inline(always)]
    fn get_node(&self, key: &str) -> Arc<InfosetNode> {
        if let Some(node) = self.nodes.get(key) {
            return Arc::clone(&node);
        }
        self.nodes
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(InfosetNode::new(HOLDEM_NUM_ACTIONS)))
            .clone()
    }

    /* Run iterations of parallel Bucketed External Sampling MCCFR */
    pub fn train(&self, iterations: u64, threads: usize, log_every: u64) {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .stack_size(8 * 1024 * 1024)
            .build_global()
            .unwrap_or(());

        let nodes = Arc::clone(&self.nodes);
        let chunk_size = if log_every > 0 { log_every } else { iterations };
        let width = iterations.to_string().len();
        let mut done = 0u64;
        let train_start = std::time::Instant::now();
        let warmup = iterations / 10;
        let train_iters_after_warmup = (iterations.saturating_sub(warmup).max(1)) as f64;

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
                let linear_weight = if iter_idx < warmup {
                    0.0
                } else {
                    (iter_idx - warmup + 1) as f64 / train_iters_after_warmup
                };
                solver_ref.traverse(&game, updating_player, linear_weight, &mut rng);
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
        weight: f64,
        rng: &mut SmallRng,
    ) -> f64 {
        if game.is_terminal() {
            return game.get_returns()[updating_player];
        }

        let curr_player = game.current_player();
        let mut actions = [0u8; HOLDEM_NUM_ACTIONS];
        let n = game.legal_actions_buf(&mut actions);
        if n == 0 {
            return 0.0;
        }

        let mut key_buf = String::with_capacity(16);
        format_holdem_infoset_key(
            &game.hole[curr_player],
            &game.board,
            game.round,
            &game.history,
            &mut key_buf,
        );
        let node = self.get_node(&key_buf);
        let mut strategy = [0.0f64; HOLDEM_NUM_ACTIONS];
        node.get_strategy_buf(&mut strategy);

        let mut legal_probs = [0.0f64; HOLDEM_NUM_ACTIONS];
        let mut prob_sum = 0.0f64;
        for i in 0..n {
            let p = strategy[actions[i] as usize];
            legal_probs[i] = p;
            prob_sum += p;
        }
        if prob_sum > 0.0 {
            let inv_sum = 1.0 / prob_sum;
            for p in legal_probs.iter_mut().take(n) {
                *p *= inv_sum;
            }
        } else {
            let uniform = 1.0 / n as f64;
            for p in legal_probs.iter_mut().take(n) {
                *p = uniform;
            }
        }

        if curr_player == updating_player {
            let mut action_utils = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut node_util = 0.0f64;

            for i in 0..n {
                let act = actions[i];
                let child = game.apply_action(act);
                let u = self.traverse(&child, updating_player, weight, rng);
                action_utils[i] = u;
                node_util += legal_probs[i] * u;
            }

            let mut regrets = [0.0f64; HOLDEM_NUM_ACTIONS];
            for i in 0..n {
                regrets[actions[i] as usize] = action_utils[i] - node_util;
            }

            node.update_regrets_cfr_plus(&regrets);
            node_util
        } else {
            let chosen_idx = sample_action_buf(&legal_probs[..n], rng);
            let chosen_act = actions[chosen_idx];

            if weight > 0.0 {
                let mut full_strategy = [0.0f64; HOLDEM_NUM_ACTIONS];
                for i in 0..n {
                    full_strategy[actions[i] as usize] = legal_probs[i];
                }
                node.accumulate_strategy(&full_strategy, weight);
            }

            let child = game.apply_action(chosen_act);
            self.traverse(&child, updating_player, weight, rng)
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

#[inline(always)]
fn sample_action_buf(probs: &[f64], rng: &mut SmallRng) -> usize {
    let r: f64 = rng.gen();
    let mut cumulative = 0.0;
    for (i, &p) in probs.iter().enumerate() {
        cumulative += p;
        if r < cumulative {
            return i;
        }
    }
    probs.len().saturating_sub(1)
}
