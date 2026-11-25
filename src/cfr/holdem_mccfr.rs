/*! Parallel External Sampling MCCFR Solver for 52-Card Texas Hold'em with Card Abstraction. */

use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use rayon::prelude::*;
use std::hash::{BuildHasherDefault, Hasher};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use crate::cfr::abstraction::hash_from_infoset_key_str;
use crate::cfr::flat_table::FlatInfosetTable;
use crate::cfr::node::InfosetNode;
use crate::cfr::train_state::{EpisodeCtx, deal_9_cards};
use crate::game::config::{
    BIG_BLIND, CALL_CHECK, FOLD, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER, ROUND_TURN, STACK_SIZE,
};

pub use crate::cfr::train_state::{StepResult, TrainState};
pub use crate::game::config::HOLDEM_NUM_ACTIONS;

const PARALLEL_CHUNK_ITERS: u64 = 2048;

fn has_ext_ignore_case(path: &str, ext: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn prefers_json_path(path: &str) -> bool {
    has_ext_ignore_case(path, "json") || path.contains(".json.") || path.contains(".tmp.json")
}

/** Fast 64-bit SplitMix mixer for pre-hashed FNV-1a `u64` keys (avoids 20-cycle SipHash-1-3 on every lookup). */
#[derive(Default)]
pub struct FastU64Hasher(u64);

impl Hasher for FastU64Hasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
        self.0 = h;
    }

    #[inline(always)]
    fn write_u64(&mut self, i: u64) {
        let mut x = i;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x ^= x >> 27;
        self.0 = x;
    }
}

pub type FastU64BuildHasher = BuildHasherDefault<FastU64Hasher>;

#[derive(Clone, Copy)]
struct TraversalParams {
    updating_player: usize,
    weight: f64,
    allow_prune: bool,
}

pub struct InfosetEntry {
    pub key: String,
    pub node: Box<InfosetNode>,
}

pub struct HoldemMCCFRSolver {
    table: Arc<FlatInfosetTable>,
    pub rich_history: bool,
    pub pruning: bool,
    pub dcfr: bool,
    pub stack_size: i32,
    pub multi_stack: bool,
    pub short_stacks_only: bool,
    pub board_texture: bool,
    pub extended_abstraction: bool,
    pub global_iterations: AtomicU64,
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
        Self::with_options(rich_history, pruning, dcfr, 4_194_304, 4096)
    }

    pub fn with_options(
        rich_history: bool,
        pruning: bool,
        dcfr: bool,
        capacity: usize,
        shard_amount: usize,
    ) -> Self {
        HoldemMCCFRSolver {
            table: Arc::new(FlatInfosetTable::new(capacity, shard_amount)),
            rich_history,
            pruning,
            dcfr,
            stack_size: STACK_SIZE,
            multi_stack: false,
            short_stacks_only: false,
            board_texture: false,
            extended_abstraction: false,
            global_iterations: AtomicU64::new(0),
        }
    }

    pub fn with_stack_size(mut self, stack_size: i32) -> Self {
        self.stack_size = stack_size.max(BIG_BLIND * 2);
        self
    }

    pub fn with_multi_stack(mut self, multi_stack: bool) -> Self {
        self.multi_stack = multi_stack;
        self
    }

    pub fn with_short_stacks_only(mut self, short_stacks_only: bool) -> Self {
        self.short_stacks_only = short_stacks_only;
        self
    }

    pub fn with_board_texture(mut self, board_texture: bool) -> Self {
        self.board_texture = board_texture;
        self
    }

    pub fn with_extended_abstraction(mut self, extended_abstraction: bool) -> Self {
        self.extended_abstraction = extended_abstraction;
        if extended_abstraction {
            self.multi_stack = true;
            self.board_texture = true;
        }
        self
    }

    pub fn num_infosets(&self) -> usize {
        self.table.len()
    }

    /** Run iterations of parallel Bucketed External Sampling MCCFR */
    pub fn train(&self, iterations: u64, threads: usize, log_every: u64) {
        self.train_with_checkpoint(iterations, threads, log_every, 0, |_, _| {});
    }

    /** Run iterations of parallel Bucketed External Sampling MCCFR with optional checkpointing */
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
        let repair_short_only =
            self.short_stacks_only || std::env::var("LIL_REPAIR_SHORT_STACKS").is_ok();
        let base_iters = self.global_iterations.load(AtomicOrdering::Relaxed);
        let total_target = base_iters + iterations;
        let dcfr_epoch = (iterations / 20).clamp(50_000, 10_000_000);
        let chunk_size = match (log_every, checkpoint_every) {
            (0, 0) => iterations.min(dcfr_epoch),
            (0, c) => c.min(dcfr_epoch),
            (l, 0) => l.min(dcfr_epoch),
            (l, c) => l.min(c).min(dcfr_epoch),
        }
        .max(1);
        let width = iterations.to_string().len();
        let mut done = 0u64;
        let mut last_checkpoint = 0u64;
        let mut last_log = 0u64;
        let mut last_discount = 0u64;
        let train_start = std::time::Instant::now();
        let warmup = (total_target / 20).clamp(1_000, 500_000);
        let train_iters_after_warmup = (total_target.saturating_sub(warmup).max(1)) as f64;
        let run_seed_base = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0xcafe_babe_dead_beef, |d| d.as_nanos() as u64);

        while done < iterations {
            let batch_end = (done + chunk_size).min(iterations);
            let batch_count = batch_end - done;
            let num_subchunks = batch_count.div_ceil(PARALLEL_CHUNK_ITERS);
            let batch_start = std::time::Instant::now();

            (0..num_subchunks).into_par_iter().for_each(|sub_idx| {
                let rel_start = sub_idx * PARALLEL_CHUNK_ITERS;
                let rel_end = (rel_start + PARALLEL_CHUNK_ITERS).min(batch_count);
                let start_global = base_iters + done + rel_start;
                let end_global = base_iters + done + rel_end;
                let rng_seed = run_seed_base
                    ^ start_global.wrapping_mul(0x9e37_79b9_7f4a_7c15)
                    ^ sub_idx.wrapping_mul(0xbf58_476d_1ce4_e5b9);
                let mut rng = SmallRng::seed_from_u64(rng_seed);

                let mut g_idx = start_global;
                while g_idx < end_global {
                    let deal_pair_idx = g_idx / 2;
                    let ep_stack = if self.extended_abstraction {
                        if repair_short_only {
                            match deal_pair_idx % 3 {
                                0 => BIG_BLIND * 15,
                                1 => BIG_BLIND * 25,
                                _ => BIG_BLIND * 45,
                            }
                        } else {
                            match deal_pair_idx % 20 {
                                0..=2 => BIG_BLIND * 15,
                                3..=6 => BIG_BLIND * 25,
                                7..=11 => BIG_BLIND * 45,
                                _ => STACK_SIZE,
                            }
                        }
                    } else if repair_short_only {
                        match deal_pair_idx % 2 {
                            0 => BIG_BLIND * 15,
                            _ => BIG_BLIND * 30,
                        }
                    } else if self.multi_stack {
                        match deal_pair_idx % 4 {
                            0 => BIG_BLIND * 15,
                            1 => BIG_BLIND * 30,
                            _ => STACK_SIZE,
                        }
                    } else {
                        self.stack_size
                    };

                    let (hole, runout) = deal_9_cards(&mut rng);
                    let ep = EpisodeCtx::new_with_mode(
                        hole,
                        runout,
                        ep_stack,
                        self.extended_abstraction,
                    );
                    let root = TrainState::new_hand(ep.stack_size);

                    if (g_idx & 1) == 0 && g_idx + 1 < end_global {
                        for p in 0..2usize {
                            let cur_g = g_idx + p as u64;
                            let weight =
                                Self::iter_weight(is_dcfr, cur_g, warmup, train_iters_after_warmup);
                            let allow_prune =
                                self.pruning && cur_g >= warmup && rng.random::<f64>() < 0.95;
                            let params = TraversalParams {
                                updating_player: p,
                                weight,
                                allow_prune,
                            };
                            self.traverse_fast(&root, &ep, params, &mut rng);
                        }
                        g_idx += 2;
                    } else {
                        let p = (g_idx & 1) as usize;
                        let weight =
                            Self::iter_weight(is_dcfr, g_idx, warmup, train_iters_after_warmup);
                        let allow_prune =
                            self.pruning && g_idx >= warmup && rng.random::<f64>() < 0.95;
                        let params = TraversalParams {
                            updating_player: p,
                            weight,
                            allow_prune,
                        };
                        self.traverse_fast(&root, &ep, params, &mut rng);
                        g_idx += 1;
                    }
                }
            });

            done = batch_end;
            self.global_iterations
                .store(base_iters + done, AtomicOrdering::Relaxed);

            if is_dcfr && (done - last_discount >= dcfr_epoch || done == iterations) {
                last_discount = done;
                let epoch = ((base_iters + done) / dcfr_epoch.max(1)).max(1) as f64;
                let e_alpha = epoch.powf(1.5);
                let pos_disc = e_alpha / (e_alpha + 1.0);
                let neg_disc = (epoch / (epoch + 1.0)).max(0.75);
                let strat_disc = (epoch / (epoch + 1.0)).powi(2);
                self.table
                    .discount_all(pos_disc, neg_disc, strat_disc, repair_short_only);
            }

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
                    done,
                    iterations,
                    pct,
                    self.table.len(),
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
    fn iter_weight(is_dcfr: bool, global_idx: u64, warmup: u64, after_warmup: f64) -> f64 {
        if is_dcfr {
            1.0
        } else if global_idx < warmup {
            (global_idx + 1) as f64 / (warmup as f64 * 100.0)
        } else {
            (global_idx - warmup + 1) as f64 / after_warmup
        }
    }

    #[inline(always)]
    fn eval_step(
        &self,
        step_res: StepResult,
        ep: &EpisodeCtx,
        params: TraversalParams,
        rng: &mut SmallRng,
    ) -> f64 {
        match step_res {
            StepResult::Continue(child) => self.traverse_fast(&child, ep, params, rng),
            StepResult::FoldWin {
                loser_contrib,
                loser,
            } => {
                let c = loser_contrib as f64;
                if params.updating_player == loser {
                    -c
                } else {
                    c
                }
            }
            StepResult::Showdown { contributions } => {
                let outcome = ep.showdown_outcome();
                let pot = contributions[0] + contributions[1];
                if outcome > 0 {
                    if params.updating_player == 0 {
                        (pot - contributions[0]) as f64
                    } else {
                        -(contributions[1] as f64)
                    }
                } else if outcome < 0 {
                    if params.updating_player == 0 {
                        -(contributions[0] as f64)
                    } else {
                        (pot - contributions[1]) as f64
                    }
                } else {
                    0.0
                }
            }
        }
    }

    fn traverse_fast(
        &self,
        state: &TrainState,
        ep: &EpisodeCtx,
        params: TraversalParams,
        rng: &mut SmallRng,
    ) -> f64 {
        let mut actions = [0u8; HOLDEM_NUM_ACTIONS];
        let n = state.legal_actions_buf(ep.stack_size, &mut actions);
        if n == 0 {
            return 0.0;
        }

        let curr_player = state.current_player as usize;
        let bucket = ep.get_bucket(state.round, curr_player);
        let tex_char = if self.board_texture && state.round == ROUND_FLOP {
            Some(ep.get_flop_texture())
        } else if self.extended_abstraction && state.round == ROUND_TURN {
            Some(ep.get_turn_texture())
        } else {
            None
        };

        let rich_hist = self.rich_history;
        let node = if let Some(pf_cache_idx) = state.preflop_cache_idx(bucket, ep.stack_tag) {
            self.table.get_or_insert_preflop(
                pf_cache_idx,
                || state.infoset_hash_tagged(bucket, None, ep.stack_tag),
                || {
                    state.format_infoset_key_tagged(
                        ep.pf_name[curr_player],
                        bucket,
                        None,
                        ep.stack_tag,
                        rich_hist,
                    )
                },
            )
        } else {
            let hash = state.infoset_hash_tagged(bucket, tex_char, ep.stack_tag);
            self.table.get_or_insert(hash, || {
                state.format_infoset_key_tagged(
                    ep.pf_name[curr_player],
                    bucket,
                    tex_char,
                    ep.stack_tag,
                    rich_hist,
                )
            })
        };

        let mut legal_probs = [0.0f64; HOLDEM_NUM_ACTIONS];
        let mut raw_regrets = [0i64; HOLDEM_NUM_ACTIONS];
        self.table.legal_strategy_and_regrets_buf(
            &node,
            &actions[..n],
            &mut legal_probs,
            &mut raw_regrets,
        );

        if curr_player == params.updating_player {
            let mut action_utils = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut node_util = 0.0f64;
            let mut explored = [true; HOLDEM_NUM_ACTIONS];

            let pot_scaled = i64::from(state.contributions[0] + state.contributions[1]) * 3_500_000;
            let r_thresh_scaled = -pot_scaled.max(ep.stack_floor_scaled);
            let opp_all_in = state.contributions[1 - curr_player] >= ep.stack_size;
            let can_prune_node = params.allow_prune && self.dcfr && state.round < ROUND_RIVER;

            for i in 0..n {
                let act = actions[i];
                if can_prune_node
                    && act != FOLD
                    && legal_probs[i] < 0.005
                    && raw_regrets[i] < r_thresh_scaled
                    && !(act == CALL_CHECK && opp_all_in)
                {
                    explored[i] = false;
                    continue;
                }

                let step_res = state.step(act, ep.stack_size, ep.is_wet_flop, rich_hist);
                let u = self.eval_step(step_res, ep, params, rng);
                action_utils[i] = u;
                node_util += legal_probs[i] * u;
            }

            self.table.update_regrets_explored(
                &node,
                &actions[..n],
                explored,
                &action_utils,
                node_util,
                self.dcfr,
            );
            node_util
        } else {
            let chosen_idx = sample_action_buf(&legal_probs[..n], rng);
            let chosen_act = actions[chosen_idx];

            if params.weight > 0.0 {
                self.table.accumulate_legal_strategy(
                    &node,
                    &actions[..n],
                    &legal_probs,
                    params.weight,
                );
            }

            let step_res = state.step(chosen_act, ep.stack_size, ep.is_wet_flop, rich_hist);
            self.eval_step(step_res, ep, params, rng)
        }
    }

    pub fn export_strategy(&self) -> std::collections::HashMap<String, Vec<f64>> {
        let mut out = std::collections::HashMap::with_capacity(self.table.len());
        self.table.for_each_average(|key, avg| {
            out.insert(key.to_string(), avg.to_vec());
        });
        out
    }

    /** Fast f32-quantized fixed-array strategy export */
    pub fn export_strategy_f32(
        &self,
    ) -> std::collections::HashMap<String, [f32; HOLDEM_NUM_ACTIONS]> {
        self.export_strategy_filtered_f32(false, 0)
    }

    /** Strategy export with optional river pruning and turn action pruning */
    pub fn export_strategy_filtered_f32(
        &self,
        prune_river: bool,
        max_turn_actions: usize,
    ) -> std::collections::HashMap<String, [f32; HOLDEM_NUM_ACTIONS]> {
        let mut out = std::collections::HashMap::with_capacity(self.table.len());
        self.table.for_each_average(|key, avg| {
            if prune_river || max_turn_actions > 0 {
                let street = if key.starts_with("P:") && !key.contains('|') {
                    ROUND_PREFLOP
                } else if key.contains("F:B") {
                    ROUND_FLOP
                } else if key.contains("T:B") {
                    ROUND_TURN
                } else if key.contains("R:B") {
                    ROUND_RIVER
                } else {
                    0
                };

                match street {
                    ROUND_PREFLOP | ROUND_FLOP => {}
                    ROUND_TURN => {
                        if max_turn_actions > 0
                            && let Some(pos) = key.find("T:B")
                        {
                            let after = &key[pos..];
                            if let Some(slash) = after.find('/') {
                                let turn_act_len = after[slash + 1..].len();
                                if turn_act_len > max_turn_actions {
                                    return;
                                }
                            }
                        }
                    }
                    ROUND_RIVER if prune_river => return,
                    _ => {}
                }
            }

            let mut f32_arr = [0.0f32; HOLDEM_NUM_ACTIONS];
            for i in 0..HOLDEM_NUM_ACTIONS {
                f32_arr[i] = avg[i] as f32;
            }
            out.insert(key.to_string(), f32_arr);
        });
        out
    }

    pub fn save_strategy_to_file_filtered(
        &self,
        path: &str,
        prune_river: bool,
        max_turn_actions: usize,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if let Some(parent) = std::path::Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        let strategy_f32 = self.export_strategy_filtered_f32(prune_river, max_turn_actions);
        let count = strategy_f32.len();
        let is_bin = !prefers_json_path(path);
        if is_bin {
            bincode::serialize_into(&mut writer, &strategy_f32)?;
        } else {
            let rounded: std::collections::HashMap<String, Vec<f64>> = strategy_f32
                .into_iter()
                .map(|(k, v)| {
                    let r: Vec<f64> = v
                        .iter()
                        .map(|&x| ((x as f64) * 10000.0).round() / 10000.0)
                        .collect();
                    (k, r)
                })
                .collect();
            serde_json::to_writer_pretty(&mut writer, &rounded)?;
        }
        writer.flush()?;
        Ok(count)
    }

    pub fn save_strategy_to_file(&self, path: &str) -> Result<usize, Box<dyn std::error::Error>> {
        if let Some(parent) = std::path::Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(path)?;
        let writer = std::io::BufWriter::new(file);
        let is_bin = !prefers_json_path(path);
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
        let prefer_json = prefers_json_path(path);

        if !prefer_json {
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

    /** Save full raw checkpoint (`hashes`, `keys`, `regret_sum`, `strategy_sum`, `global_iterations`) for lossless resume */
    pub fn save_raw_checkpoint(&self, path: &str) -> Result<usize, Box<dyn std::error::Error>> {
        if let Some(parent) = std::path::Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        let global_iters = self.global_iterations.load(AtomicOrdering::Relaxed);
        let entries = self.table.collect_raw_entries();
        let count = entries.len();
        bincode::serialize_into(&mut writer, &(global_iters, entries))?;
        writer.flush()?;
        Ok(count)
    }

    /** Warm-start solver nodes from a raw checkpoint or strategy file (for resuming/extending training) */
    pub fn warm_start_from_file(&self, path: &str) -> Result<usize, Box<dyn std::error::Error>> {
        type RawCkpt = (
            u64,
            Vec<(
                u64,
                String,
                [i64; HOLDEM_NUM_ACTIONS],
                [i64; HOLDEM_NUM_ACTIONS],
            )>,
        );
        let raw_ckpt_path = format!("{path}.ckpt");
        let try_raw_path = if std::path::Path::new(&raw_ckpt_path).exists() {
            Some(raw_ckpt_path.as_str())
        } else if has_ext_ignore_case(path, "ckpt") {
            Some(path)
        } else {
            None
        };

        if let Some(rp) = try_raw_path
            && let Ok(file) = std::fs::File::open(rp)
        {
            let mut reader = std::io::BufReader::new(file);
            if let Ok((saved_iters, entries)) = bincode::deserialize_from::<_, RawCkpt>(&mut reader)
            {
                let count = entries.len();
                self.global_iterations
                    .store(saved_iters, AtomicOrdering::Relaxed);
                for (h, key, r, s) in entries {
                    self.table.insert_raw_state(h, key, &r, &s);
                }
                return Ok(count);
            }
        }

        let strat_map = Self::load_strategy_from_file(path)?;
        let count = strat_map.len();
        for (key, probs) in strat_map {
            let h = hash_from_infoset_key_str(&key);
            self.table.insert_warm_probs(h, key, &probs);
        }
        Ok(count)
    }
}

#[inline(always)]
fn sample_action_buf(probs: &[f64], rng: &mut SmallRng) -> usize {
    let r: f64 = rng.random();
    let mut cumulative = 0.0;
    for (i, &p) in probs.iter().enumerate() {
        cumulative += p;
        if r < cumulative {
            return i;
        }
    }
    probs.len().saturating_sub(1)
}
