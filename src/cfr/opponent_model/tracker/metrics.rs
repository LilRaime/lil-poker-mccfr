/*! Derived Opponent Statistical Ratios, Bayesian Estimators, and Style Classification */
use super::super::classifier::OpponentStyle;
use super::OpponentTracker;

impl OpponentTracker {
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

    pub fn vpip_ratio(&self) -> f64 {
        if self.total_hands == 0 && self.vpip_hands == 0 {
            0.50
        } else {
            let denom = self.total_hands.max(self.vpip_hands).max(1);
            (self.vpip_hands as f64 / denom as f64).min(1.0)
        }
    }

    pub fn pfr_ratio(&self) -> f64 {
        if self.total_hands == 0 && self.pfr_hands == 0 {
            0.25
        } else {
            let denom = self.total_hands.max(self.pfr_hands).max(1);
            (self.pfr_hands as f64 / denom as f64).min(1.0)
        }
    }

    pub fn flop_fold_to_cbet_ratio(&self) -> f64 {
        if self.flop_cbet_faced == 0 {
            0.40
        } else {
            self.flop_fold_to_cbet as f64 / self.flop_cbet_faced as f64
        }
    }

    pub fn turn_fold_to_cbet_ratio(&self) -> f64 {
        if self.turn_cbet_faced == 0 {
            0.40
        } else {
            self.turn_fold_to_cbet as f64 / self.turn_cbet_faced as f64
        }
    }

    pub fn river_fold_to_bet_ratio(&self) -> f64 {
        if self.river_bet_faced == 0 {
            0.35
        } else {
            self.river_fold_to_bet as f64 / self.river_bet_faced as f64
        }
    }

    pub fn wtsd_ratio(&self) -> f64 {
        if self.saw_flop == 0 {
            0.30
        } else {
            self.went_to_showdown as f64 / self.saw_flop as f64
        }
    }

    pub fn sb_vpip_ratio(&self) -> f64 {
        if self.sb_total_hands == 0 {
            0.70
        } else {
            self.sb_vpip_hands as f64 / self.sb_total_hands as f64
        }
    }

    pub fn sb_pfr_ratio(&self) -> f64 {
        if self.sb_total_hands == 0 {
            0.50
        } else {
            self.sb_pfr_hands as f64 / self.sb_total_hands as f64
        }
    }

    pub fn bb_vpip_ratio(&self) -> f64 {
        if self.bb_total_hands == 0 {
            0.55
        } else {
            self.bb_vpip_hands as f64 / self.bb_total_hands as f64
        }
    }

    pub fn fold_to_3bet_ratio(&self) -> f64 {
        if self.faced_3bet_count == 0 {
            0.50
        } else {
            self.fold_to_3bet_count as f64 / self.faced_3bet_count as f64
        }
    }

    pub fn sb_limp_ratio(&self) -> f64 {
        if self.sb_total_hands == 0 {
            0.0
        } else {
            (self.sb_vpip_hands.saturating_sub(self.sb_pfr_hands)) as f64
                / self.sb_total_hands as f64
        }
    }

    pub fn flop_check_ratio(&self) -> f64 {
        let checks = if self.flop_checks > 0 || self.flop_cbet_faced > 0 {
            self.flop_checks
        } else {
            self.flop_actions[1]
        };
        let total = checks + self.flop_actions[2];
        if total == 0 {
            0.50
        } else {
            checks as f64 / total as f64
        }
    }

    pub fn postflop_overbet_ratio(&self) -> f64 {
        if self.postflop_bets_total == 0 {
            0.0
        } else {
            self.postflop_overbets as f64 / self.postflop_bets_total as f64
        }
    }

    pub fn is_overbet_bluffer(&self) -> bool {
        self.exploit_enabled
            && self.postflop_bets_total >= 4
            && self.postflop_overbets >= 3
            && self.postflop_overbet_ratio() >= 0.40
    }

    /** Bayesian Beta-prior smoothed estimators (population prior + pseudo-count k)
     * to prevent abrupt archetype flips on small samples (5–20 hands). */
    pub fn smoothed_vpip(&self) -> f64 {
        let k = 8.0;
        let prior = 0.55;
        let n = self.total_hands.max(self.vpip_hands) as f64;
        ((self.vpip_hands as f64 + prior * k) / (n + k)).clamp(0.0, 1.0)
    }

    pub fn smoothed_pfr(&self) -> f64 {
        let k = 8.0;
        let prior = 0.32;
        let n = self.total_hands.max(self.pfr_hands) as f64;
        ((self.pfr_hands as f64 + prior * k) / (n + k)).clamp(0.0, 1.0)
    }

    pub fn smoothed_fold_ratio(&self) -> f64 {
        let k = 12.0;
        let prior = 0.33;
        let n = self.total_actions() as f64;
        ((self.fold_count as f64 + prior * k) / (n + k)).clamp(0.0, 1.0)
    }

    pub fn smoothed_call_ratio(&self) -> f64 {
        let k = 12.0;
        let prior = 0.42;
        let n = self.total_actions() as f64;
        ((self.call_count as f64 + prior * k) / (n + k)).clamp(0.0, 1.0)
    }

    pub fn smoothed_raise_ratio(&self) -> f64 {
        let k = 12.0;
        let prior = 0.25;
        let n = self.total_actions() as f64;
        ((self.raise_count as f64 + prior * k) / (n + k)).clamp(0.0, 1.0)
    }

    pub fn smoothed_flop_fold_to_cbet(&self) -> f64 {
        let k = 5.0;
        let prior = 0.42;
        let n = self.flop_cbet_faced as f64;
        ((self.flop_fold_to_cbet as f64 + prior * k) / (n + k)).clamp(0.0, 1.0)
    }

    pub fn smoothed_turn_fold_to_cbet(&self) -> f64 {
        let k = 5.0;
        let prior = 0.40;
        let n = self.turn_cbet_faced as f64;
        ((self.turn_fold_to_cbet as f64 + prior * k) / (n + k)).clamp(0.0, 1.0)
    }

    pub fn smoothed_river_fold_to_bet(&self) -> f64 {
        let k = 5.0;
        let prior = 0.38;
        let n = self.river_bet_faced as f64;
        ((self.river_fold_to_bet as f64 + prior * k) / (n + k)).clamp(0.0, 1.0)
    }

    pub fn showdown_bluff_rate(&self) -> f64 {
        if self.showdown_hands == 0 {
            0.20
        } else {
            self.showdown_bluff_count as f64 / self.showdown_hands as f64
        }
    }

    /** Wilson score interval for statistical significance of frequency bounds.
     * Returns (`lower_bound`, `upper_bound`) at confidence level z (e.g. z=1.96 for 95%). */
    pub fn wilson_confidence(successes: u64, total: u64, z: f64) -> (f64, f64) {
        if total == 0 {
            return (0.0, 1.0);
        }
        let n = total as f64;
        let p_hat = successes as f64 / n;
        let z2 = z * z;
        let denom = 1.0 + z2 / n;
        let center = (p_hat + z2 / (2.0 * n)) / denom;
        let margin = (z / denom)
            * ((p_hat * (1.0 - p_hat) / n) + (z2 / (4.0 * n * n)))
                .max(0.0)
                .sqrt();
        ((center - margin).max(0.0), (center + margin).min(1.0))
    }

    /** Classifies opponent playstyle into a discrete archetype with confidence */
    pub fn classify_style(&self) -> (OpponentStyle, f64) {
        if self.total_hands < 3 {
            return (OpponentStyle::Unknown, 0.0);
        }
        let vpip = self.vpip_ratio();
        let pfr = self.pfr_ratio();
        let fold = self.fold_ratio();
        let raise = self.raise_ratio();

        let conf = (self.total_hands as f64 / (self.total_hands as f64 + 7.0)).min(0.95);

        if pfr > 0.38 && raise > 0.35 {
            (OpponentStyle::Maniac, conf)
        } else if vpip > 0.60 && pfr < 0.20 && fold < 0.28 {
            (OpponentStyle::CallingStation, conf)
        } else if vpip < 0.22 && fold > 0.45 {
            (OpponentStyle::Rock, conf)
        } else if (0.18..=0.35).contains(&vpip) && (pfr >= 0.14 || raise >= 0.15) {
            (OpponentStyle::Tag, conf)
        } else if vpip <= 0.35 && fold >= 0.40 {
            (OpponentStyle::Rock, conf * 0.85)
        } else if (0.35..=0.65).contains(&vpip) || (vpip > 0.65 && pfr >= 0.25) {
            (OpponentStyle::Lag, conf)
        } else {
            (OpponentStyle::CallingStation, conf * 0.7)
        }
    }
}
