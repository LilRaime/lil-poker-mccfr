/* Opponent Action Tracking and Exploitative Policy Generator. */

#[derive(Debug, Clone)]
pub struct OpponentTracker {
    pub total_hands: u64,
    pub vpip_hands: u64,
    pub fold_count: u64,
    pub call_count: u64,
    pub raise_count: u64,
}

impl Default for OpponentTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl OpponentTracker {
    pub fn new() -> Self {
        OpponentTracker {
            total_hands: 0,
            vpip_hands: 0,
            fold_count: 0,
            call_count: 0,
            raise_count: 0,
        }
    }

    pub fn record_action(&mut self, action: u8, is_preflop: bool) {
        match action {
            0 => self.fold_count += 1,
            1 => {
                self.call_count += 1;
                if is_preflop {
                    self.vpip_hands += 1;
                }
            }
            2 | 3 => {
                self.raise_count += 1;
                if is_preflop {
                    self.vpip_hands += 1;
                }
            }
            _ => {}
        }
    }

    pub fn end_hand(&mut self) {
        self.total_hands += 1;
    }

    pub fn total_actions(&self) -> u64 {
        self.fold_count + self.call_count + self.raise_count
    }

    pub fn fold_ratio(&self) -> f64 {
        let tot = self.total_actions();
        if tot == 0 {
            0.33
        } else {
            self.fold_count as f64 / tot as f64
        }
    }

    pub fn call_ratio(&self) -> f64 {
        let tot = self.total_actions();
        if tot == 0 {
            0.33
        } else {
            self.call_count as f64 / tot as f64
        }
    }

    pub fn raise_ratio(&self) -> f64 {
        let tot = self.total_actions();
        if tot == 0 {
            0.33
        } else {
            self.raise_count as f64 / tot as f64
        }
    }

    /* Blends Blueprint action probabilities with an exploitative adjustment. */
    pub fn adjust_strategy(&self, blueprint_probs: &[f64], legal_actions: &[u8]) -> Vec<f64> {
        let n = blueprint_probs.len();
        if self.total_actions() < 5 {
            return blueprint_probs.to_vec();
        }

        let mut adjusted = blueprint_probs.to_vec();
        let fold_rate = self.fold_ratio();

        if fold_rate > 0.50 {
            /* Opponent over-folds: boost raise actions, reduce fold */
            for (idx, &a) in legal_actions.iter().enumerate() {
                if a == 2 || a == 3 {
                    adjusted[idx] *= 1.35;
                } else if a == 0 {
                    adjusted[idx] *= 0.70;
                }
            }
        } else if fold_rate < 0.20 && self.call_ratio() > 0.50 {
            /* Calling station opponent: increase value bets */
            for (idx, &a) in legal_actions.iter().enumerate() {
                if a == 1 {
                    adjusted[idx] *= 1.25;
                }
            }
        }

        let sum: f64 = adjusted.iter().sum();
        if sum > 0.0 {
            for p in adjusted.iter_mut() {
                *p /= sum;
            }
        } else {
            return vec![1.0 / n as f64; n];
        }

        adjusted
    }
}
