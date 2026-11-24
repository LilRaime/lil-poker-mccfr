/*!
 * Cache-line-aligned (`64 B`), lock-free Structure-of-Arrays (SoA) open-addressing infoset table
 * for parallel Hold'em MCCFR/DCFR training.
 *
 * Separates hot regret accumulators (`RegretSlot`, 64 B) from opponent-only strategy sums
 * (`StratSlot`, 64 B) and cold key strings (`DashMap`), eliminating per-lookup shard `RwLock`
 * atomic traffic and `Box<InfosetNode>` pointer chasing while keeping an unbounded overflow
 * `DashMap` fallback if linear probing ever exceeds `MAX_PROBE`.
 */

use dashmap::DashMap;
use rayon::prelude::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};

use crate::cfr::holdem_mccfr::{FastU64BuildHasher, InfosetEntry};
use crate::cfr::node::{InfosetNode, REGRET_FLOOR_I64, SCALE_F64};
use crate::game::config::HOLDEM_NUM_ACTIONS;

const MAX_PROBE: usize = 64;
const NUM_OCCUPIED_SHARDS: usize = 64;
pub(crate) const PF_SEQ_CACHE_STRIDE: usize = 781;
const PF_SLOT_CACHE_LEN: usize = 3 * 169 * PF_SEQ_CACHE_STRIDE;

#[repr(C, align(64))]
pub(crate) struct RegretSlot {
    pub(crate) hash: AtomicU64,
    pub(crate) regret_sum: [AtomicI64; HOLDEM_NUM_ACTIONS],
    pub(crate) _pad: AtomicU64,
}

#[repr(C, align(64))]
pub(crate) struct StratSlot {
    pub(crate) strategy_sum: [AtomicI64; HOLDEM_NUM_ACTIONS],
    pub(crate) _pad: [u64; 2],
}

const _: () = assert!(std::mem::size_of::<RegretSlot>() == 64);
const _: () = assert!(std::mem::size_of::<StratSlot>() == 64);

#[inline(always)]
fn non_zero_tag(hash: u64) -> u64 {
    if hash == 0 { 1 } else { hash }
}

#[inline(always)]
fn slot_index_mix(hash: u64) -> usize {
    let mut x = hash;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (x ^ (x >> 31)) as usize
}

fn alloc_zeroed_slice<T>(len: usize) -> Box<[T]> {
    assert!(
        len > 0 && std::mem::size_of::<T>() > 0,
        "slice length and element size must be non-zero"
    );
    let layout = std::alloc::Layout::array::<T>(len).expect("valid aligned slice layout");
    /* SAFETY: `T` is `RegretSlot` or `StratSlot` (`#[repr(C, align(64))]` composed solely of
     * `AtomicU64`, `AtomicI64`, and `[u64; 2]`), for all of which an all-zero bit pattern is a
     * valid initialized state. OS zero-pages remain unmapped in physical RAM until written. */
    unsafe {
        let raw = std::alloc::alloc_zeroed(layout);
        if raw.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        let slice_ptr = std::ptr::slice_from_raw_parts_mut(raw.cast::<T>(), len);
        Box::from_raw(slice_ptr)
    }
}

pub(crate) enum NodeRef<'a> {
    Flat(usize),
    Overflow(&'a InfosetNode),
}

pub(crate) struct FlatInfosetTable {
    regret_slots: Box<[RegretSlot]>,
    strat_slots: Box<[StratSlot]>,
    pf_slot_cache: Box<[AtomicU32]>,
    mask: usize,
    keys: DashMap<usize, (u64, String), FastU64BuildHasher>,
    occupied_shards: Box<[Mutex<Vec<u32>>]>,
    overflow: DashMap<u64, InfosetEntry, FastU64BuildHasher>,
}

impl FlatInfosetTable {
    pub(crate) fn new(capacity: usize, shard_amount: usize) -> Self {
        let cap = capacity.clamp(65_536, 16_777_216).next_power_of_two();
        let map_shards = shard_amount.clamp(64, 4096).next_power_of_two();
        let shards_vec: Vec<Mutex<Vec<u32>>> = (0..NUM_OCCUPIED_SHARDS)
            .map(|_| Mutex::new(Vec::with_capacity(cap / (NUM_OCCUPIED_SHARDS * 2))))
            .collect();
        let pf_cache: Vec<AtomicU32> = (0..PF_SLOT_CACHE_LEN)
            .map(|_| AtomicU32::new(u32::MAX))
            .collect();

        Self {
            regret_slots: alloc_zeroed_slice::<RegretSlot>(cap),
            strat_slots: alloc_zeroed_slice::<StratSlot>(cap),
            pf_slot_cache: pf_cache.into_boxed_slice(),
            mask: cap - 1,
            keys: DashMap::with_capacity_and_hasher_and_shard_amount(
                (cap / 2).min(2_097_152),
                FastU64BuildHasher::default(),
                map_shards,
            ),
            occupied_shards: shards_vec.into_boxed_slice(),
            overflow: DashMap::with_capacity_and_hasher_and_shard_amount(
                1024,
                FastU64BuildHasher::default(),
                64,
            ),
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.keys.len() + self.overflow.len()
    }

    #[inline(always)]
    pub(crate) fn get_or_insert_preflop<H, F>(
        &self,
        cache_idx: usize,
        compute_hash: H,
        make_key: F,
    ) -> NodeRef<'_>
    where
        H: FnOnce() -> u64,
        F: FnOnce() -> String,
    {
        let cached = self.pf_slot_cache[cache_idx].load(Ordering::Relaxed);
        if cached != u32::MAX {
            return NodeRef::Flat(cached as usize);
        }
        let node = self.get_or_insert(compute_hash(), make_key);
        if let NodeRef::Flat(slot_idx) = node {
            self.pf_slot_cache[cache_idx].store(slot_idx as u32, Ordering::Relaxed);
        }
        node
    }

    #[inline(always)]
    pub(crate) fn get_or_insert<F>(&self, hash: u64, make_key: F) -> NodeRef<'_>
    where
        F: FnOnce() -> String,
    {
        let tag = non_zero_tag(hash);
        let mut idx = slot_index_mix(tag) & self.mask;

        for _ in 0..MAX_PROBE {
            let slot = &self.regret_slots[idx];
            let cur = slot.hash.load(Ordering::Relaxed);
            if cur == tag {
                return NodeRef::Flat(idx);
            }
            if cur == 0 {
                match slot
                    .hash
                    .compare_exchange(0, tag, Ordering::AcqRel, Ordering::Relaxed)
                {
                    Ok(_) => {
                        let key = make_key();
                        self.keys.insert(idx, (hash, key));
                        if let Ok(mut shard) =
                            self.occupied_shards[idx & (NUM_OCCUPIED_SHARDS - 1)].lock()
                        {
                            shard.push(idx as u32);
                        }
                        return NodeRef::Flat(idx);
                    }
                    Err(actual) if actual == tag => {
                        return NodeRef::Flat(idx);
                    }
                    Err(_) => {}
                }
            }
            idx = (idx + 1) & self.mask;
        }

        self.get_or_insert_overflow(hash, make_key)
    }

    #[cold]
    fn get_or_insert_overflow<F>(&self, hash: u64, make_key: F) -> NodeRef<'_>
    where
        F: FnOnce() -> String,
    {
        if let Some(entry) = self.overflow.get(&hash) {
            let ptr: *const InfosetNode = &*entry.node;
            /* SAFETY: `self.overflow` is append-only while `&self` is borrowed during training,
             * and `Box<InfosetNode>` keeps a stable heap address across `DashMap` resizes. */
            return NodeRef::Overflow(unsafe { &*ptr });
        }
        let entry = self.overflow.entry(hash).or_insert_with(|| InfosetEntry {
            key: make_key(),
            node: Box::new(InfosetNode::new(HOLDEM_NUM_ACTIONS)),
        });
        let ptr: *const InfosetNode = &*entry.node;
        /* SAFETY: Same pinned `Box<InfosetNode>` invariant as above. */
        NodeRef::Overflow(unsafe { &*ptr })
    }

    #[inline(always)]
    pub(crate) fn legal_strategy_and_regrets_buf(
        &self,
        node: &NodeRef<'_>,
        legal_actions: &[u8],
        out_probs: &mut [f64; HOLDEM_NUM_ACTIONS],
        out_raw_regrets: &mut [i64; HOLDEM_NUM_ACTIONS],
    ) {
        match *node {
            NodeRef::Flat(idx) => {
                let slot = &self.regret_slots[idx];
                let n = legal_actions.len();
                let mut total = 0.0f64;
                for i in 0..n {
                    let raw = slot.regret_sum[legal_actions[i] as usize].load(Ordering::Relaxed);
                    out_raw_regrets[i] = raw;
                    let r = raw.max(0) as f64;
                    out_probs[i] = r;
                    total += r;
                }
                if total > 0.0 {
                    let inv = 1.0 / total;
                    for item in out_probs.iter_mut().take(n) {
                        *item *= inv;
                    }
                } else {
                    let uniform = 1.0 / n as f64;
                    for item in out_probs.iter_mut().take(n) {
                        *item = uniform;
                    }
                }
            }
            NodeRef::Overflow(n_ref) => {
                n_ref.get_legal_strategy_buf(legal_actions, out_probs);
                for (i, &act) in legal_actions.iter().enumerate() {
                    out_raw_regrets[i] = (n_ref.get_regret(act as usize) * SCALE_F64) as i64;
                }
            }
        }
    }

    #[inline(always)]
    pub(crate) fn update_regrets_explored(
        &self,
        node: &NodeRef<'_>,
        actions: &[u8],
        explored: [bool; HOLDEM_NUM_ACTIONS],
        action_utils: &[f64; HOLDEM_NUM_ACTIONS],
        node_util: f64,
        is_dcfr: bool,
    ) {
        match *node {
            NodeRef::Flat(idx) => {
                let slot = &self.regret_slots[idx];
                if is_dcfr {
                    for (i, &act) in actions.iter().enumerate() {
                        if explored[i] {
                            let delta = ((action_utils[i] - node_util) * SCALE_F64) as i64;
                            if delta != 0 {
                                slot.regret_sum[act as usize].fetch_add(delta, Ordering::Relaxed);
                            }
                        }
                    }
                } else {
                    for (i, &act) in actions.iter().enumerate() {
                        if explored[i] {
                            let delta = ((action_utils[i] - node_util) * SCALE_F64) as i64;
                            let atom = &slot.regret_sum[act as usize];
                            let mut old = atom.load(Ordering::Relaxed);
                            loop {
                                let new = (old + delta).max(0);
                                match atom.compare_exchange_weak(
                                    old,
                                    new,
                                    Ordering::Relaxed,
                                    Ordering::Relaxed,
                                ) {
                                    Ok(_) => break,
                                    Err(actual) => old = actual,
                                }
                            }
                        }
                    }
                }
            }
            NodeRef::Overflow(n_ref) => {
                let mut regrets = [0.0f64; HOLDEM_NUM_ACTIONS];
                for (i, &act) in actions.iter().enumerate() {
                    if explored[i] {
                        regrets[act as usize] = action_utils[i] - node_util;
                    }
                }
                if is_dcfr {
                    n_ref.update_regrets_linear(&regrets);
                } else {
                    n_ref.update_regrets_cfr_plus(&regrets);
                }
            }
        }
    }

    #[inline(always)]
    pub(crate) fn accumulate_legal_strategy(
        &self,
        node: &NodeRef<'_>,
        actions: &[u8],
        legal_probs: &[f64; HOLDEM_NUM_ACTIONS],
        weight: f64,
    ) {
        match *node {
            NodeRef::Flat(idx) => {
                let strat = &self.strat_slots[idx];
                let scale_w = weight * SCALE_F64;
                for (i, &act) in actions.iter().enumerate() {
                    let delta = (legal_probs[i] * scale_w) as i64;
                    if delta != 0 {
                        strat.strategy_sum[act as usize].fetch_add(delta, Ordering::Relaxed);
                    }
                }
            }
            NodeRef::Overflow(n_ref) => {
                let mut full = [0.0f64; HOLDEM_NUM_ACTIONS];
                for (i, &act) in actions.iter().enumerate() {
                    full[act as usize] = legal_probs[i];
                }
                n_ref.accumulate_strategy(&full, weight);
            }
        }
    }

    pub(crate) fn discount_all(
        &self,
        pos_disc: f64,
        neg_disc: f64,
        strat_disc: f64,
        short_stacks_only: bool,
    ) {
        let do_strat = (strat_disc - 1.0).abs() > 1e-9;
        self.occupied_shards.par_iter().for_each(|shard_mutex| {
            let Ok(indices) = shard_mutex.lock() else {
                return;
            };
            for &raw_idx in indices.iter() {
                let idx = raw_idx as usize;
                if short_stacks_only {
                    let is_short = self.keys.get(&idx).is_some_and(|e| {
                        let k = &e.value().1;
                        k.starts_with("S15|") || k.starts_with("S30|")
                    });
                    if !is_short {
                        continue;
                    }
                }
                let r_slot = &self.regret_slots[idx];
                let s_slot = &self.strat_slots[idx];
                for a in 0..HOLDEM_NUM_ACTIONS {
                    let r = r_slot.regret_sum[a].load(Ordering::Relaxed);
                    if r != 0 {
                        let disc = if r > 0 {
                            (r as f64 * pos_disc) as i64
                        } else {
                            (r as f64 * neg_disc) as i64
                        };
                        r_slot.regret_sum[a].store(disc.max(REGRET_FLOOR_I64), Ordering::Relaxed);
                    }
                    if do_strat {
                        let s = s_slot.strategy_sum[a].load(Ordering::Relaxed);
                        if s != 0 {
                            let s_d = (s as f64 * strat_disc) as i64;
                            s_slot.strategy_sum[a].store(s_d.max(0), Ordering::Relaxed);
                        }
                    }
                }
            }
        });

        if !self.overflow.is_empty() {
            self.overflow.iter().par_bridge().for_each(|entry| {
                if short_stacks_only {
                    let k = &entry.value().key;
                    if !k.starts_with("S15|") && !k.starts_with("S30|") {
                        return;
                    }
                }
                entry.value().node.discount(pos_disc, neg_disc, strat_disc);
            });
        }
    }

    pub(crate) fn insert_raw_state(
        &self,
        hash: u64,
        key: String,
        regrets: &[i64; HOLDEM_NUM_ACTIONS],
        strategies: &[i64; HOLDEM_NUM_ACTIONS],
    ) {
        let tag = non_zero_tag(hash);
        let mut idx = slot_index_mix(tag) & self.mask;

        for _ in 0..MAX_PROBE {
            let slot = &self.regret_slots[idx];
            let cur = slot.hash.load(Ordering::Acquire);
            if cur == tag {
                for a in 0..HOLDEM_NUM_ACTIONS {
                    slot.regret_sum[a].store(regrets[a], Ordering::Relaxed);
                    self.strat_slots[idx].strategy_sum[a].store(strategies[a], Ordering::Relaxed);
                }
                self.keys.insert(idx, (hash, key));
                return;
            }
            if cur == 0
                && slot
                    .hash
                    .compare_exchange(0, tag, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                for a in 0..HOLDEM_NUM_ACTIONS {
                    slot.regret_sum[a].store(regrets[a], Ordering::Relaxed);
                    self.strat_slots[idx].strategy_sum[a].store(strategies[a], Ordering::Relaxed);
                }
                self.keys.insert(idx, (hash, key));
                if let Ok(mut shard) = self.occupied_shards[idx & (NUM_OCCUPIED_SHARDS - 1)].lock()
                {
                    shard.push(idx as u32);
                }
                return;
            }
            idx = (idx + 1) & self.mask;
        }

        let node = Box::new(InfosetNode::new(HOLDEM_NUM_ACTIONS));
        node.set_raw_state(regrets, strategies);
        self.overflow.insert(hash, InfosetEntry { key, node });
    }

    pub(crate) fn insert_warm_probs(&self, hash: u64, key: String, probs: &[f64]) {
        let mut r = [0i64; HOLDEM_NUM_ACTIONS];
        let mut s = [0i64; HOLDEM_NUM_ACTIONS];
        for (i, &p) in probs.iter().take(HOLDEM_NUM_ACTIONS).enumerate() {
            r[i] = ((p * 200.0) * SCALE_F64) as i64;
            s[i] = ((p * 100.0) * SCALE_F64) as i64;
        }
        self.insert_raw_state(hash, key, &r, &s);
    }

    pub(crate) fn for_each_average<F>(&self, mut visit: F)
    where
        F: FnMut(&str, [f64; HOLDEM_NUM_ACTIONS]),
    {
        for entry in &self.keys {
            let idx = *entry.key();
            let key_str = &entry.value().1;
            let strat_slot = &self.strat_slots[idx];
            let mut sums = [0.0f64; HOLDEM_NUM_ACTIONS];
            let mut total = 0.0f64;
            for (a, item) in sums.iter_mut().enumerate() {
                let val = (strat_slot.strategy_sum[a].load(Ordering::Relaxed) as f64 / SCALE_F64)
                    .max(0.0);
                *item = val;
                total += val;
            }
            if total > 0.0 {
                let inv = 1.0 / total;
                for item in &mut sums {
                    *item *= inv;
                }
            } else {
                let u = 1.0 / HOLDEM_NUM_ACTIONS as f64;
                sums = [u; HOLDEM_NUM_ACTIONS];
            }
            visit(key_str, sums);
        }

        for entry in &self.overflow {
            let avg_vec = entry.value().node.get_average_strategy();
            let mut arr = [0.0f64; HOLDEM_NUM_ACTIONS];
            for (a, &v) in avg_vec.iter().take(HOLDEM_NUM_ACTIONS).enumerate() {
                arr[a] = v;
            }
            visit(&entry.value().key, arr);
        }
    }

    pub(crate) fn collect_raw_entries(
        &self,
    ) -> Vec<(
        u64,
        String,
        [i64; HOLDEM_NUM_ACTIONS],
        [i64; HOLDEM_NUM_ACTIONS],
    )> {
        let mut out = Vec::with_capacity(self.len());
        for entry in &self.keys {
            let idx = *entry.key();
            let (orig_hash, ref key_str) = *entry.value();
            let mut r = [0i64; HOLDEM_NUM_ACTIONS];
            let mut s = [0i64; HOLDEM_NUM_ACTIONS];
            for a in 0..HOLDEM_NUM_ACTIONS {
                r[a] = self.regret_slots[idx].regret_sum[a].load(Ordering::Relaxed);
                s[a] = self.strat_slots[idx].strategy_sum[a].load(Ordering::Relaxed);
            }
            out.push((orig_hash, key_str.clone(), r, s));
        }
        for entry in &self.overflow {
            let (r, s) = entry.value().node.get_raw_state();
            out.push((*entry.key(), entry.value().key.clone(), r, s));
        }
        out
    }
}
