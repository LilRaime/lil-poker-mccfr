/*
 * Lock-free information set node for parallel MCCFR.
 * Stores regret and strategy sums as scaled integers (× SCALE) in AtomicI64 using fetch_add.
 */
use std::sync::atomic::{AtomicI64, Ordering};

/* Fixed-point scale: 1.0 → 1_000_000 in integer storage. */
const SCALE: f64 = 1_000_000.0;

pub struct InfosetNode {
    pub num_actions: usize,
    regret_sum: Box<[AtomicI64]>,
    strategy_sum: Box<[AtomicI64]>,
}

impl InfosetNode {
    pub fn new(num_actions: usize) -> Self {
        let make = |_| AtomicI64::new(0);
        InfosetNode {
            num_actions,
            regret_sum: (0..num_actions)
                .map(make)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            strategy_sum: (0..num_actions)
                .map(make)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }

    /* Current strategy via regret matching (positive part normalised). */
    pub fn get_strategy(&self) -> Vec<f64> {
        let regrets: Vec<f64> = self
            .regret_sum
            .iter()
            .map(|r| (r.load(Ordering::Relaxed) as f64 / SCALE).max(0.0))
            .collect();

        let total: f64 = regrets.iter().sum();
        if total > 0.0 {
            regrets.iter().map(|&r| r / total).collect()
        } else {
            vec![1.0 / self.num_actions as f64; self.num_actions]
        }
    }

    /* Average strategy (used as final policy after training). */
    pub fn get_average_strategy(&self) -> Vec<f64> {
        let sums: Vec<f64> = self
            .strategy_sum
            .iter()
            .map(|s| (s.load(Ordering::Relaxed) as f64 / SCALE).max(0.0))
            .collect();
        let total: f64 = sums.iter().sum();
        if total > 0.0 {
            sums.iter().map(|&s| s / total).collect()
        } else {
            vec![1.0 / self.num_actions as f64; self.num_actions]
        }
    }

    /* Add regrets for each action using fetch_add (CFR+ clamp applied lazily on read). */
    pub fn update_regrets_cfr_plus(&self, regrets: &[f64]) {
        for (i, &r) in regrets.iter().enumerate() {
            let delta = (r * SCALE) as i64;
            self.regret_sum[i].fetch_add(delta, Ordering::Relaxed);
        }
    }

    /* Accumulate strategy (weighted by iteration index for linear averaging). */
    pub fn accumulate_strategy(&self, strategy: &[f64], weight: f64) {
        for (i, &s) in strategy.iter().enumerate() {
            let delta = (s * weight * SCALE) as i64;
            self.strategy_sum[i].fetch_add(delta, Ordering::Relaxed);
        }
    }
}

/* InfosetNode is Sync because all interior mutation is atomic. */
unsafe impl Sync for InfosetNode {}
