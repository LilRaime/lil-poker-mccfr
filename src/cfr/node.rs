/*
 * Lock-free information set node for parallel MCCFR.
 * Stores regret and strategy sums as scaled integers (× SCALE) in AtomicI64.
 * Uses atomic CAS clamping for true CFR+ (regrets floored at 0).
 */
use std::sync::atomic::{AtomicI64, Ordering};

/* Fixed-point scale: 1.0 → 1_000_000 in integer storage. */
const SCALE: f64 = 1_000_000.0;
pub const MAX_ACTIONS: usize = crate::game::config::HOLDEM_NUM_ACTIONS;
pub const REGRET_FLOOR_I64: i64 = -200_000_000_000;
pub const REGRET_FLOOR_F64: f64 = REGRET_FLOOR_I64 as f64 / SCALE;

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
            regret_sum: std::array::from_fn(|_| AtomicI64::new(0)),
            strategy_sum: std::array::from_fn(|_| AtomicI64::new(0)),
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

    /* Cumulative regret for a specific action */
    #[inline(always)]
    pub fn get_regret(&self, action: usize) -> f64 {
        if action < self.num_actions {
            self.regret_sum[action].load(Ordering::Relaxed) as f64 / SCALE
        } else {
            0.0
        }
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

    /* Fast lock-free regret update via fetch_add (used in epoch-based DCFR / Linear CFR). */
    #[inline(always)]
    pub fn update_regrets_linear(&self, regrets: &[f64]) {
        for (i, &r) in regrets.iter().take(self.num_actions).enumerate() {
            let delta = (r * SCALE) as i64;
            if delta != 0 {
                self.regret_sum[i].fetch_add(delta, Ordering::Relaxed);
            }
        }
    }

    /* Epoch-level DCFR / Linear CFR discount and floor clamping pass across all actions. */
    #[inline]
    pub fn discount(&self, pos_discount: f64, neg_discount: f64, strat_discount: f64) {
        for i in 0..self.num_actions {
            let r = self.regret_sum[i].load(Ordering::Relaxed);
            if r != 0 {
                let discounted = if r > 0 {
                    (r as f64 * pos_discount) as i64
                } else {
                    (r as f64 * neg_discount) as i64
                };
                self.regret_sum[i].store(discounted.max(REGRET_FLOOR_I64), Ordering::Relaxed);
            }
            if (strat_discount - 1.0).abs() > 1e-9 {
                let s = self.strategy_sum[i].load(Ordering::Relaxed);
                if s != 0 {
                    let s_disc = (s as f64 * strat_discount) as i64;
                    self.strategy_sum[i].store(s_disc.max(0), Ordering::Relaxed);
                }
            }
        }
    }

    /* Export raw atomic state for lossless training checkpoints. */
    pub fn get_raw_state(&self) -> ([i64; MAX_ACTIONS], [i64; MAX_ACTIONS]) {
        let mut r = [0i64; MAX_ACTIONS];
        let mut s = [0i64; MAX_ACTIONS];
        for i in 0..self.num_actions {
            r[i] = self.regret_sum[i].load(Ordering::Relaxed);
            s[i] = self.strategy_sum[i].load(Ordering::Relaxed);
        }
        (r, s)
    }

    /* Restore raw atomic state from a training checkpoint. */
    pub fn set_raw_state(&self, regrets: &[i64; MAX_ACTIONS], strategies: &[i64; MAX_ACTIONS]) {
        for i in 0..self.num_actions {
            self.regret_sum[i].store(regrets[i], Ordering::Relaxed);
            self.strategy_sum[i].store(strategies[i], Ordering::Relaxed);
        }
    }

    /* Discounted CFR (DCFR, Brown & Sandholm 2019):
     * Discounts positive regrets by pos_discount = t^alpha / (t^alpha + 1),
     * and negative regrets by neg_discount = t^beta / (t^beta + 1). */
    #[inline(always)]
    pub fn update_regrets_dcfr(&self, regrets: &[f64], pos_discount: f64, neg_discount: f64) {
        if (pos_discount - 1.0).abs() < 1e-9 && (neg_discount - 1.0).abs() < 1e-9 {
            self.update_regrets_linear(regrets);
            return;
        }
        for (i, &r) in regrets.iter().take(self.num_actions).enumerate() {
            let delta = (r * SCALE) as i64;
            let mut old = self.regret_sum[i].load(Ordering::Relaxed);
            loop {
                let current_r = old as f64;
                let discounted = if current_r > 0.0 {
                    current_r * pos_discount
                } else if current_r < 0.0 {
                    current_r * neg_discount
                } else {
                    0.0
                };
                let new = (discounted as i64)
                    .saturating_add(delta)
                    .max(REGRET_FLOOR_I64);
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
