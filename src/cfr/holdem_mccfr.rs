/* Parallel External Sampling MCCFR Solver for 52-Card Texas Hold'em with Card Abstraction. */

use dashmap::DashMap;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use crate::cfr::node::InfosetNode;
use crate::game::holdem::TexasHoldemGame;

pub use crate::game::config::HOLDEM_NUM_ACTIONS;

pub struct InfosetEntry {
    pub key: String,
    pub node: Box<InfosetNode>,
}

pub struct HoldemMCCFRSolver {
    pub nodes: Arc<DashMap<u64, InfosetEntry>>,
    pub rich_history: bool,
    pub pruning: bool,
    pub dcfr: bool,
}

impl Default for HoldemMCCFRSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl HoldemMCCFRSolver {
    pub fn new() -> Self {
        Self::with_config(false, true, true)
    }

    pub fn with_config(rich_history: bool, pruning: bool, dcfr: bool) -> Self {
        Self::with_options(rich_history, pruning, dcfr, 1_000_000, 2048)
    }

    pub fn with_options(
        rich_history: bool,
        pruning: bool,
        dcfr: bool,
        capacity: usize,
        shard_amount: usize,
    ) -> Self {
        HoldemMCCFRSolver {
            nodes: Arc::new(DashMap::with_capacity_and_shard_amount(
                capacity,
                shard_amount,
            )),
            rich_history,
            pruning,
            dcfr,
        }
    }

    /* Fast get or create node by u64 hash; key string generated lazily only on insertion */
    #[inline(always)]
    fn get_node<F>(&self, hash: u64, make_key: F) -> &InfosetNode
    where
        F: FnOnce() -> String,
    {
        if let Some(entry) = self.nodes.get(&hash) {
            let ptr: *const InfosetNode = &*entry.node;
            return unsafe { &*ptr };
        }
        let entry = self.nodes.entry(hash).or_insert_with(|| InfosetEntry {
            key: make_key(),
            node: Box::new(InfosetNode::new(HOLDEM_NUM_ACTIONS)),
        });
        let ptr: *const InfosetNode = &*entry.node;
        unsafe { &*ptr }
    }

    /* Run iterations of parallel Bucketed External Sampling MCCFR */
    pub fn train(&self, iterations: u64, threads: usize, log_every: u64) {
        self.train_with_checkpoint(iterations, threads, log_every, 0, |_, _| {});
    }

    /* Run iterations of parallel Bucketed External Sampling MCCFR with optional checkpointing */
    pub fn train_with_checkpoint<F>(
        &self,
        iterations: u64,
        threads: usize,
        log_every: u64,
        checkpoint_every: u64,
        mut on_checkpoint: F,
    ) where
        F: FnMut(u64, &HoldemMCCFRSolver),
    {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .stack_size(8 * 1024 * 1024)
            .build_global()
            .unwrap_or(());

        let is_dcfr = self.dcfr;
        let chunk_size = match (log_every, checkpoint_every) {
            (0, 0) => iterations,
            (0, c) => c,
            (l, 0) => l,
            (l, c) => l.min(c),
        };
        let width = iterations.to_string().len();
        let mut done = 0u64;
        let mut last_checkpoint = 0u64;
        let mut last_log = 0u64;
        let train_start = std::time::Instant::now();
        let warmup = (iterations / 10).min(500_000);
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
            let batch_count = batch_end - done;
            let entropy_ref = Arc::clone(&entropy_base);

            let batch_start = std::time::Instant::now();

            batch.into_par_iter().for_each(|iter_idx| {
                let thread_entropy = entropy_ref.fetch_add(1, AtomicOrdering::Relaxed);
                let seed = iter_idx.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ thread_entropy;
                let mut rng = SmallRng::seed_from_u64(seed);
                let updating_player = (iter_idx % 2) as usize;
                let game = TexasHoldemGame::new_random(&mut rng);
                let (pos_disc, neg_disc, weight) = if is_dcfr {
                    let t = (iter_idx + 1) as f64;
                    let t_alpha = t.powf(1.5);
                    let p_disc = t_alpha / (t_alpha + 1.0);
                    let t_beta = t.powf(0.5);
                    let n_disc = t_beta / (t_beta + 1.0);
                    let w = (t / iterations as f64).powf(2.0);
                    (p_disc, n_disc, w)
                } else {
                    let linear_weight = if iter_idx < warmup {
                        /* During warmup: accumulate with tiny linear ramp to start building strategy without bias */
                        (iter_idx + 1) as f64 / (warmup as f64 * 100.0)
                    } else {
                        (iter_idx - warmup + 1) as f64 / train_iters_after_warmup
                    };
                    (1.0, 0.0, linear_weight)
                };
                self.traverse(&game, updating_player, weight, pos_disc, neg_disc, &mut rng);
            });

            done = batch_end;

            if log_every > 0 && (done - last_log >= log_every || done == iterations) {
                last_log = done;
                let elapsed = train_start.elapsed().as_secs_f64();
                let batch_secs = batch_start.elapsed().as_secs_f64().max(1e-9);
                let iter_per_s = batch_count as f64 / batch_secs;
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
                    self.nodes.len(),
                    iter_per_s / 1000.0,
                    fmt_secs(elapsed),
                    fmt_secs(eta_secs),
                    width = width,
                );
            }

            if checkpoint_every > 0
                && done < iterations
                && (done - last_checkpoint >= checkpoint_every)
            {
                last_checkpoint = done;
                on_checkpoint(done, self);
            }
        }

        eprintln!();
    }

    #[inline(always)]
    fn traverse(
        &self,
        game: &TexasHoldemGame,
        updating_player: usize,
        weight: f64,
        pos_discount: f64,
        neg_discount: f64,
        rng: &mut SmallRng,
    ) -> f64 {
        self.traverse_internal(
            game,
            updating_player,
            weight,
            pos_discount,
            neg_discount,
            rng,
            [0, 0],
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn traverse_internal(
        &self,
        game: &TexasHoldemGame,
        updating_player: usize,
        weight: f64,
        pos_discount: f64,
        neg_discount: f64,
        rng: &mut SmallRng,
        cached_buckets: [u8; 2],
        cached_round: u8,
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

        let (buckets, buckets_round) = if game.round > 1 && game.round != cached_round {
            let b0 =
                crate::cfr::abstraction::postflop_equity_bucket(&game.hole[0], &game.board) as u8;
            let b1 =
                crate::cfr::abstraction::postflop_equity_bucket(&game.hole[1], &game.board) as u8;
            ([b0, b1], game.round)
        } else {
            (cached_buckets, cached_round)
        };

        let node = if self.rich_history {
            let hash = if game.round > 1 {
                crate::cfr::abstraction::hash_holdem_infoset_key_rich_with_bucket(
                    &game.hole[curr_player],
                    buckets[curr_player],
                    game.round,
                    &game.history,
                )
            } else {
                crate::cfr::abstraction::hash_holdem_infoset_key_rich(
                    &game.hole[curr_player],
                    &game.board,
                    game.round,
                    &game.history,
                )
            };
            self.get_node(hash, || {
                crate::cfr::abstraction::get_holdem_infoset_key_rich_with_bucket(
                    &game.hole[curr_player],
                    buckets[curr_player],
                    game.round,
                    &game.history,
                )
            })
        } else {
            let hash = crate::cfr::abstraction::hash_holdem_infoset_key_with_bucket(
                &game.hole[curr_player],
                buckets[curr_player],
                game.round,
                &game.history,
            );
            self.get_node(hash, || {
                crate::cfr::abstraction::get_holdem_infoset_key_with_bucket(
                    &game.hole[curr_player],
                    buckets[curr_player],
                    game.round,
                    &game.history,
                )
            })
        };
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
            let mut explored = [true; HOLDEM_NUM_ACTIONS];

            let r_thresh = -200.0;
            let p_prune = 0.95;

            for i in 0..n {
                let act = actions[i];
                let act_idx = act as usize;

                if self.pruning {
                    let r = node.get_regret(act_idx);
                    let should_prune = if self.dcfr {
                        r < r_thresh
                    } else {
                        weight > 200.0 && r <= 0.0 && legal_probs[i] < 0.01
                    };
                    if should_prune && rng.gen::<f64>() < p_prune {
                        explored[i] = false;
                        continue;
                    }
                }

                let child = game.apply_action(act);
                let u = self.traverse_internal(
                    &child,
                    updating_player,
                    weight,
                    pos_discount,
                    neg_discount,
                    rng,
                    buckets,
                    buckets_round,
                );
                action_utils[i] = u;
                node_util += legal_probs[i] * u;
            }

            let mut regrets = [0.0f64; HOLDEM_NUM_ACTIONS];
            for i in 0..n {
                let act_idx = actions[i] as usize;
                if explored[i] {
                    regrets[act_idx] = action_utils[i] - node_util;
                } else {
                    regrets[act_idx] = 0.0;
                }
            }

            if self.dcfr {
                node.update_regrets_dcfr(&regrets, pos_discount, neg_discount);
            } else {
                node.update_regrets_cfr_plus(&regrets);
            }
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
            self.traverse_internal(
                &child,
                updating_player,
                weight,
                pos_discount,
                neg_discount,
                rng,
                buckets,
                buckets_round,
            )
        }
    }

    pub fn export_strategy(&self) -> std::collections::HashMap<String, Vec<f64>> {
        self.nodes
            .iter()
            .map(|entry| {
                let key = entry.value().key.clone();
                let avg = entry.value().node.get_average_strategy();
                (key, avg)
            })
            .collect()
    }

    /* Fast f32-quantized fixed-array strategy export */
    pub fn export_strategy_f32(
        &self,
    ) -> std::collections::HashMap<String, [f32; HOLDEM_NUM_ACTIONS]> {
        self.export_strategy_filtered_f32(false, 0)
    }

    /* Strategy export with optional river pruning and turn action pruning */
    pub fn export_strategy_filtered_f32(
        &self,
        prune_river: bool,
        max_turn_actions: usize,
    ) -> std::collections::HashMap<String, [f32; HOLDEM_NUM_ACTIONS]> {
        self.nodes
            .iter()
            .filter_map(|entry| {
                let key = &entry.value().key;
                if prune_river || max_turn_actions > 0 {
                    let street = if key.starts_with("P:") && !key.contains('|') {
                        1
                    } else if key.contains("F:B") {
                        2
                    } else if key.contains("T:B") {
                        3
                    } else if key.contains("R:B") {
                        4
                    } else {
                        0
                    };

                    match street {
                        1 => {}
                        2 => {
                            if key.starts_with("P:") {
                                if let Some(end) = key.find('|') {
                                    if key[2..end].len() > 4 {
                                        return None;
                                    }
                                }
                            }
                        }
                        3 => {
                            if key.starts_with("P:") {
                                if let Some(end) = key.find('|') {
                                    if key[2..end].len() > 4 {
                                        return None;
                                    }
                                }
                            }
                            if max_turn_actions > 0 {
                                if let Some(pos) = key.find("T:B") {
                                    let after = &key[pos..];
                                    if let Some(slash) = after.find('/') {
                                        let turn_act_len = after[slash + 1..].len();
                                        if turn_act_len > max_turn_actions {
                                            return None;
                                        }
                                    }
                                }
                            }
                        }
                        4 if prune_river => return None,
                        _ => {}
                    }
                }

                let avg = entry.value().node.get_average_strategy();
                let mut f32_arr = [0.0f32; HOLDEM_NUM_ACTIONS];
                for i in 0..HOLDEM_NUM_ACTIONS {
                    f32_arr[i] = avg[i] as f32;
                }
                Some((key.clone(), f32_arr))
            })
            .collect()
    }

    pub fn save_strategy_to_file_filtered(
        &self,
        path: &str,
        prune_river: bool,
        max_turn_actions: usize,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        let strategy_f32 = self.export_strategy_filtered_f32(prune_river, max_turn_actions);
        let count = strategy_f32.len();
        bincode::serialize_into(&mut writer, &strategy_f32)?;
        use std::io::Write;
        writer.flush()?;
        Ok(count)
    }

    pub fn save_strategy_to_file(&self, path: &str) -> Result<usize, Box<dyn std::error::Error>> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = std::fs::File::create(path)?;
        let writer = std::io::BufWriter::new(file);
        let is_bin =
            !path.ends_with(".json") && !path.contains(".json.") && !path.contains(".tmp.json");
        let node_count = if is_bin {
            let strategy_f32 = self.export_strategy_f32();
            let count = strategy_f32.len();
            bincode::serialize_into(writer, &strategy_f32)?;
            count
        } else {
            let strategy = self.export_strategy();
            let count = strategy.len();
            let rounded: std::collections::HashMap<String, Vec<f64>> = strategy
                .into_iter()
                .map(|(k, v)| {
                    let r: Vec<f64> = v.iter().map(|x| (x * 10000.0).round() / 10000.0).collect();
                    (k, r)
                })
                .collect();
            serde_json::to_writer_pretty(writer, &rounded)?;
            count
        };
        Ok(node_count)
    }

    pub fn load_strategy_from_file(
        path: &str,
    ) -> Result<std::collections::HashMap<String, Vec<f64>>, Box<dyn std::error::Error>> {
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let mut prefix = [0u8; 1];
        let n = file.read(&mut prefix)?;
        if n == 0 {
            return Err("Empty strategy file".into());
        }
        let is_json_content =
            prefix[0] == b'{' || prefix[0] == b'[' || prefix[0] == b' ' || prefix[0] == b'\n';

        if !is_json_content {
            let file_bincode = std::fs::File::open(path)?;
            let mut reader = std::io::BufReader::new(file_bincode);
            if let Ok(raw_f32) = bincode::deserialize_from::<
                _,
                std::collections::HashMap<String, [f32; HOLDEM_NUM_ACTIONS]>,
            >(&mut reader)
            {
                let map = raw_f32
                    .into_iter()
                    .map(|(k, v)| (k, v.iter().map(|&x| x as f64).collect()))
                    .collect();
                return Ok(map);
            }
            let file_retry = std::fs::File::open(path)?;
            let reader_retry = std::io::BufReader::new(file_retry);
            if let Ok(strat) = bincode::deserialize_from::<
                _,
                std::collections::HashMap<String, Vec<f64>>,
            >(reader_retry)
            {
                return Ok(strat);
            }
        }

        let file_json = std::fs::File::open(path)?;
        let reader_json = std::io::BufReader::new(file_json);
        let strat: std::collections::HashMap<String, Vec<f64>> =
            serde_json::from_reader(reader_json)?;
        Ok(strat)
    }

    /* Warm-start solver nodes from an existing saved strategy file (for resuming/extending training) */
    pub fn warm_start_from_file(&self, path: &str) -> Result<usize, Box<dyn std::error::Error>> {
        let strat_map = Self::load_strategy_from_file(path)?;
        let count = strat_map.len();
        for (key, probs) in strat_map {
            let mut h = 0xcbf29ce484222325u64;
            for &b in key.as_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            let node = Box::new(InfosetNode::new(HOLDEM_NUM_ACTIONS));
            node.accumulate_strategy(&probs, 100.0);
            node.update_regrets_cfr_plus(&probs);
            self.nodes.insert(h, InfosetEntry { key, node });
        }
        Ok(count)
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
