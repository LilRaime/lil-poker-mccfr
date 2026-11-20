/*! Parallel External Sampling MCCFR Solver for 52-Card Texas Hold'em with Card Abstraction. */

use dashmap::DashMap;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::cell::Cell;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use crate::cfr::node::InfosetNode;
use crate::game::holdem::{Card, TexasHoldemGame};

pub use crate::game::config::HOLDEM_NUM_ACTIONS;

/** Fast 64-bit SplitMix mixer for pre-hashed FNV-1a u64 keys (avoids 20-cycle SipHash-1-3 on every lookup). */
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
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
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

struct EpisodeBucketCache<'a> {
    runout: &'a [Card; 5],
    pf: [u8; 2],
    postflop: [[Cell<u8>; 2]; 3],
    flop_tex: Cell<char>,
}

impl<'a> EpisodeBucketCache<'a> {
    #[inline(always)]
    fn new(hole: &[[Card; 2]; 2], runout: &'a [Card; 5]) -> Self {
        Self {
            runout,
            pf: [
                crate::cfr::abstraction::preflop_bucket(hole[0][0], hole[0][1]).0 as u8,
                crate::cfr::abstraction::preflop_bucket(hole[1][0], hole[1][1]).0 as u8,
            ],
            postflop: [
                [Cell::new(u8::MAX), Cell::new(u8::MAX)],
                [Cell::new(u8::MAX), Cell::new(u8::MAX)],
                [Cell::new(u8::MAX), Cell::new(u8::MAX)],
            ],
            flop_tex: Cell::new('\0'),
        }
    }

    #[inline(always)]
    fn get_bucket(&self, hole: &[[Card; 2]; 2], round: u8, player: usize) -> u8 {
        if round <= 1 {
            return self.pf[player];
        }
        let s_idx = (round - 2) as usize;
        let cached = self.postflop[s_idx][player].get();
        if cached != u8::MAX {
            return cached;
        }
        let board_len = (round as usize) + 1;
        let b = crate::cfr::abstraction::postflop_equity_bucket(
            &hole[player],
            &self.runout[..board_len],
        ) as u8;
        self.postflop[s_idx][player].set(b);
        b
    }

    #[inline(always)]
    fn get_flop_texture(&self) -> char {
        let c = self.flop_tex.get();
        if c != '\0' {
            return c;
        }
        let tc = crate::cfr::abstraction::compact_board_texture_char(&self.runout[..3]);
        self.flop_tex.set(tc);
        tc
    }
}

pub struct InfosetEntry {
    pub key: String,
    pub node: Box<InfosetNode>,
}

pub struct HoldemMCCFRSolver {
    pub nodes: Arc<DashMap<u64, InfosetEntry, FastU64BuildHasher>>,
    pub rich_history: bool,
    pub pruning: bool,
    pub dcfr: bool,
    pub stack_size: i32,
    pub multi_stack: bool,
    pub board_texture: bool,
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
        /* Pre-allocate 4M capacity across 4096 shards (~100 MB RAM) to eliminate runtime shard rehashing. */
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
            nodes: Arc::new(DashMap::with_capacity_and_hasher_and_shard_amount(
                capacity,
                FastU64BuildHasher::default(),
                shard_amount,
            )),
            rich_history,
            pruning,
            dcfr,
            stack_size: crate::game::config::STACK_SIZE,
            multi_stack: false,
            board_texture: false,
            global_iterations: AtomicU64::new(0),
        }
    }

    pub fn with_stack_size(mut self, stack_size: i32) -> Self {
        self.stack_size = stack_size.max(crate::game::config::BIG_BLIND * 2);
        self
    }

    pub fn with_multi_stack(mut self, multi_stack: bool) -> Self {
        self.multi_stack = multi_stack;
        self
    }

    pub fn with_board_texture(mut self, board_texture: bool) -> Self {
        self.board_texture = board_texture;
        self
    }

    /** Fast get or create node by u64 hash; key string generated lazily only on insertion */
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
        let repair_short_only = std::env::var("LIL_REPAIR_SHORT_STACKS").is_ok();
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
                let global_idx = base_iters + iter_idx;
                let thread_entropy = entropy_ref.fetch_add(1, AtomicOrdering::Relaxed);
                let seed = global_idx.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ thread_entropy;
                let mut rng = SmallRng::seed_from_u64(seed);
                let updating_player = (global_idx % 2) as usize;
                /* Divide by 2 before `% 4` so each stack depth (15 BB, 30 BB, 100 BB)
                 * is paired across both `updating_player == 0` and `updating_player == 1`! */
                let ep_stack = if repair_short_only {
                    match (global_idx / 2) % 2 {
                        0 => crate::game::config::BIG_BLIND * 15,
                        _ => crate::game::config::BIG_BLIND * 30,
                    }
                } else if self.multi_stack {
                    match (global_idx / 2) % 4 {
                        0 => crate::game::config::BIG_BLIND * 15,
                        1 => crate::game::config::BIG_BLIND * 30,
                        _ => crate::game::config::STACK_SIZE,
                    }
                } else {
                    self.stack_size
                };
                let game = TexasHoldemGame::new_random_with_stack(&mut rng, ep_stack);
                let runout = game.deck_remaining.peek5();
                let cache = EpisodeBucketCache::new(&game.hole, runout);

                let weight = if is_dcfr {
                    /* In epoch-discounted DCFR, each iteration accumulates at unit scale (1.0)
                     * to preserve full fixed-point precision (STRAT_SCALE = 10,000), while
                     * `node.discount(..., strat_disc)` scales prior epochs by (t / (t + 1))^2. */
                    1.0
                } else if global_idx < warmup {
                    /* During warmup: accumulate with tiny linear ramp to start building strategy without bias */
                    (global_idx + 1) as f64 / (warmup as f64 * 100.0)
                } else {
                    (global_idx - warmup + 1) as f64 / train_iters_after_warmup
                };

                let allow_prune = self.pruning && global_idx >= warmup && rng.gen::<f64>() < 0.95;
                self.traverse_precomputed(
                    &game,
                    updating_player,
                    weight,
                    allow_prune,
                    &cache,
                    &mut rng,
                );
            });

            done = batch_end;
            self.global_iterations
                .store(base_iters + done, AtomicOrdering::Relaxed);

            /* Epoch-based DCFR discounting pass across all infoset nodes (Pluribus / Brown & Sandholm 2019) */
            if is_dcfr && (done - last_discount >= dcfr_epoch || done == iterations) {
                last_discount = done;
                let epoch = ((base_iters + done) / dcfr_epoch.max(1)).max(1) as f64;
                let e_alpha = epoch.powf(1.5);
                let pos_disc = e_alpha / (e_alpha + 1.0);
                let neg_disc = (epoch / (epoch + 1.0)).max(0.75);
                let strat_disc = (epoch / (epoch + 1.0)).powi(2);
                self.nodes.iter().par_bridge().for_each(|entry| {
                    if repair_short_only {
                        let k = &entry.value().key;
                        if !k.starts_with("S15|") && !k.starts_with("S30|") {
                            return;
                        }
                    }
                    entry.value().node.discount(pos_disc, neg_disc, strat_disc);
                });
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
    fn traverse_precomputed(
        &self,
        game: &TexasHoldemGame,
        updating_player: usize,
        weight: f64,
        allow_prune: bool,
        cache: &EpisodeBucketCache<'_>,
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

        let bucket = cache.get_bucket(&game.hole, game.round, curr_player);
        /* Tag Flop (round == 2) with compact board texture ('d'/'w'/'p'/'m').
         * Skip Turn & River (rounds 3..=4) where 88 equity buckets + online SubgameSolver
         * already capture board structure, preventing a 4x postflop infoset explosion. */
        let tex_char = if self.board_texture && game.round == 2 {
            Some(cache.get_flop_texture())
        } else {
            None
        };

        let stack_prefix = crate::cfr::abstraction::stack_depth_prefix(
            game.stack_size / crate::game::config::BIG_BLIND.max(1),
        );
        let node = if self.rich_history {
            let mut hash = crate::cfr::abstraction::hash_holdem_infoset_key_rich_fast(
                bucket,
                game.round,
                &game.history,
            );
            if let Some(tc) = tex_char {
                hash = crate::cfr::abstraction::mix_texture_hash(hash, tc);
            }
            hash = crate::cfr::abstraction::mix_stack_prefix_hash(hash, stack_prefix);
            self.get_node(hash, || {
                let mut k = crate::cfr::abstraction::get_holdem_infoset_key_rich_with_bucket(
                    &game.hole[curr_player],
                    bucket,
                    game.round,
                    &game.history,
                );
                if let Some(tc) = tex_char {
                    k = crate::cfr::abstraction::append_texture_to_key(&k, tc);
                }
                if stack_prefix.is_empty() {
                    k
                } else {
                    format!("{}{}", stack_prefix, k)
                }
            })
        } else {
            let mut hash = crate::cfr::abstraction::hash_holdem_infoset_key_fast(
                bucket,
                game.round,
                &game.history,
            );
            if let Some(tc) = tex_char {
                hash = crate::cfr::abstraction::mix_texture_hash(hash, tc);
            }
            hash = crate::cfr::abstraction::mix_stack_prefix_hash(hash, stack_prefix);
            self.get_node(hash, || {
                let mut k = crate::cfr::abstraction::get_holdem_infoset_key_with_bucket(
                    &game.hole[curr_player],
                    bucket,
                    game.round,
                    &game.history,
                );
                if let Some(tc) = tex_char {
                    k = crate::cfr::abstraction::append_texture_to_key(&k, tc);
                }
                if stack_prefix.is_empty() {
                    k
                } else {
                    format!("{}{}", stack_prefix, k)
                }
            })
        };

        let mut legal_probs = [0.0f64; HOLDEM_NUM_ACTIONS];
        node.get_legal_strategy_buf(&actions[..n], &mut legal_probs);

        if curr_player == updating_player {
            let mut action_utils = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut node_util = 0.0f64;
            let mut explored = [true; HOLDEM_NUM_ACTIONS];

            /* Pluribus-style Regret-Based Pruning:
             * - Coin flip (95%) is rolled once per episode (`allow_prune`);
             * - Dynamic stack- and pot-relative threshold (-3.5 * pot, floored at 6% of stack) when prob < 0.005;
             * - Never prune FOLD or River actions (round 4) or actions that immediately terminate the hand. */
            let pot = (game.contributions[0] + game.contributions[1]) as f64;
            let stack_floor = (game.stack_size as f64 * 0.06).max(90.0);
            let r_thresh = -(pot * 3.5).max(stack_floor);
            let opp_all_in = game.contributions[1 - curr_player] >= game.stack_size;

            for i in 0..n {
                let act = actions[i];
                let act_idx = act as usize;

                if allow_prune && game.round < 4 && act != crate::game::config::FOLD {
                    let r = node.get_regret(act_idx);
                    let should_prune = self.dcfr && r < r_thresh && legal_probs[i] < 0.005;
                    if should_prune {
                        /* At round < 4 and act != FOLD, only calling an all-in can terminate the hand. */
                        if !(act == crate::game::config::CALL_CHECK && opp_all_in) {
                            explored[i] = false;
                            continue;
                        }
                    }
                }

                let child = game.apply_action(act);
                let u = self.traverse_precomputed(
                    &child,
                    updating_player,
                    weight,
                    allow_prune,
                    cache,
                    rng,
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
                node.update_regrets_linear(&regrets);
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
            self.traverse_precomputed(&child, updating_player, weight, allow_prune, cache, rng)
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
                        2 => {}
                        3 => {
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
        let is_bin =
            !path.ends_with(".json") && !path.contains(".json.") && !path.contains(".tmp.json");
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
        /* Try bincode first for non-.json paths: the first byte of a bincode HashMap is
         * `(len % 256)` in little-endian, which can legitimately equal '{' (123), '[' (91),
         * ' ' (32), or '\n' (10) and falsely trigger a 1-byte JSON sniff! */
        let prefer_json =
            path.ends_with(".json") || path.contains(".json.") || path.contains(".tmp.json");

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

    /** Save full raw checkpoint (hashes, keys, regret_sum, strategy_sum, global_iterations) for lossless resume */
    pub fn save_raw_checkpoint(&self, path: &str) -> Result<usize, Box<dyn std::error::Error>> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        let global_iters = self.global_iterations.load(AtomicOrdering::Relaxed);
        let entries: Vec<(
            u64,
            String,
            [i64; HOLDEM_NUM_ACTIONS],
            [i64; HOLDEM_NUM_ACTIONS],
        )> = self
            .nodes
            .iter()
            .map(|e| {
                let (r, s) = e.value().node.get_raw_state();
                (*e.key(), e.value().key.clone(), r, s)
            })
            .collect();
        let count = entries.len();
        bincode::serialize_into(&mut writer, &(global_iters, entries))?;
        use std::io::Write;
        writer.flush()?;
        Ok(count)
    }

    /** Warm-start solver nodes from a raw checkpoint or strategy file (for resuming/extending training) */
    pub fn warm_start_from_file(&self, path: &str) -> Result<usize, Box<dyn std::error::Error>> {
        let raw_ckpt_path = format!("{}.ckpt", path);
        let try_raw_path = if std::path::Path::new(&raw_ckpt_path).exists() {
            Some(raw_ckpt_path.as_str())
        } else if path.ends_with(".ckpt") {
            Some(path)
        } else {
            None
        };

        if let Some(rp) = try_raw_path {
            if let Ok(file) = std::fs::File::open(rp) {
                let mut reader = std::io::BufReader::new(file);
                type RawCkpt = (
                    u64,
                    Vec<(
                        u64,
                        String,
                        [i64; HOLDEM_NUM_ACTIONS],
                        [i64; HOLDEM_NUM_ACTIONS],
                    )>,
                );
                if let Ok((saved_iters, entries)) =
                    bincode::deserialize_from::<_, RawCkpt>(&mut reader)
                {
                    let count = entries.len();
                    self.global_iterations
                        .store(saved_iters, AtomicOrdering::Relaxed);
                    for (h, key, r, s) in entries {
                        let node = Box::new(InfosetNode::new(HOLDEM_NUM_ACTIONS));
                        node.set_raw_state(&r, &s);
                        self.nodes.insert(h, InfosetEntry { key, node });
                    }
                    return Ok(count);
                }
            }
        }

        let strat_map = Self::load_strategy_from_file(path)?;
        let count = strat_map.len();
        for (key, probs) in strat_map {
            let h = crate::cfr::abstraction::hash_from_infoset_key_str(&key);
            let node = Box::new(InfosetNode::new(HOLDEM_NUM_ACTIONS));
            node.accumulate_strategy(&probs, 100.0);
            let scaled_regrets: Vec<f64> = probs.iter().map(|&p| p * 200.0).collect();
            node.update_regrets_cfr_plus(&scaled_regrets);
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
