/*
 * Lock-free information set node for parallel MCCFR.
 * Stores regret and strategy sums as scaled integers (× SCALE) in AtomicI64.
 * Uses atomic CAS clamping for true CFR+ (regrets floored at 0).
 */
use std::sync::atomic::{AtomicI64, Ordering};

/* Fixed-point scale: 1.0 → 1_000_000 in integer storage. */
const SCALE: f64 = 1_000_000.0;
pub const MAX_ACTIONS: usize = 4;

pub struct InfosetNode {
    pub num_actions: usize,
    regret_sum: [AtomicI64; MAX_ACTIONS],
    strategy_sum: [AtomicI64; MAX_ACTIONS],
}

impl InfosetNode {
    pub fn new(num_actions: usize) -> Self {
        assert!(
            num_actions <= MAX_ACTIONS,
            "num_actions exceeds MAX_ACTIONS"
        );
        InfosetNode {
            num_actions,
            regret_sum: [
                AtomicI64::new(0),
                AtomicI64::new(0),
                AtomicI64::new(0),
                AtomicI64::new(0),
            ],
            strategy_sum: [
                AtomicI64::new(0),
                AtomicI64::new(0),
                AtomicI64::new(0),
                AtomicI64::new(0),
            ],
        }
    }

    /* Current strategy via regret matching (positive part normalised) into a stack buffer. */
    #[inline(always)]
    pub fn get_strategy_buf(&self, out: &mut [f64; MAX_ACTIONS]) {
        let n = self.num_actions;
        let mut total = 0.0f64;
        for (i, item) in out.iter_mut().enumerate().take(n) {
            let r = (self.regret_sum[i].load(Ordering::Relaxed) as f64 / SCALE).max(0.0);
            *item = r;
            total += r;
        }
        if total > 0.0 {
            let inv_total = 1.0 / total;
            for item in out.iter_mut().take(n) {
                *item *= inv_total;
            }
        } else {
            let uniform = 1.0 / n as f64;
            for item in out.iter_mut().take(n) {
                *item = uniform;
            }
        }
    }

    /* Current strategy via regret matching (allocated Vec). */
    pub fn get_strategy(&self) -> Vec<f64> {
        let mut buf = [0.0; MAX_ACTIONS];
        self.get_strategy_buf(&mut buf);
        buf[..self.num_actions].to_vec()
    }

    /* Average strategy (used as final policy after training). */
    pub fn get_average_strategy(&self) -> Vec<f64> {
        let n = self.num_actions;
        let mut sums = [0.0; MAX_ACTIONS];
        let mut total = 0.0f64;
        for (i, item) in sums.iter_mut().enumerate().take(n) {
            let s = (self.strategy_sum[i].load(Ordering::Relaxed) as f64 / SCALE).max(0.0);
            *item = s;
            total += s;
        }
        if total > 0.0 {
            sums[..n].iter().map(|&s| s / total).collect()
        } else {
            vec![1.0 / n as f64; n]
        }
    }

    /* Add regrets for each action using atomic CAS clamp for true CFR+ (floor at 0). */
    #[inline(always)]
    pub fn update_regrets_cfr_plus(&self, regrets: &[f64]) {
        for (i, &r) in regrets.iter().take(self.num_actions).enumerate() {
            let delta = (r * SCALE) as i64;
            let mut old = self.regret_sum[i].load(Ordering::Relaxed);
            loop {
                let new = (old + delta).max(0);
                match self.regret_sum[i].compare_exchange_weak(
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

    /* Accumulate strategy (weighted for linear averaging). */
    #[inline(always)]
    pub fn accumulate_strategy(&self, strategy: &[f64], weight: f64) {
        let scale_weight = weight * SCALE;
        for (i, &s) in strategy.iter().take(self.num_actions).enumerate() {
            let delta = (s * scale_weight) as i64;
            self.strategy_sum[i].fetch_add(delta, Ordering::Relaxed);
        }
    }
}

/* InfosetNode is Sync because all interior mutation is atomic. */
unsafe impl Sync for InfosetNode {}
