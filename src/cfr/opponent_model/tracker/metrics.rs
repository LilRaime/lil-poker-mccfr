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

    /** Voluntary non-check calls (`call_count` minus recorded postflop checks). */
    pub fn genuine_call_count(&self) -> u64 {
        self.call_count.saturating_sub(self.check_count)
    }

    /** Fraction of all tracked opponent actions that were postflop checks. */
    pub fn check_ratio(&self) -> f64 {
        let tot = self.total_actions();
        if tot == 0 {
            0.0
        } else {
            self.check_count as f64 / tot as f64
        }
    }

    /** True Aggression Frequency (AFq): `raises / (folds + calls + checks + raises)`. */
    pub fn afq_ratio(&self) -> f64 {
        self.raise_ratio()
    }

    /** True postflop Aggression Frequency (`raises / all postflop actions incl. checks`). */
    pub fn postflop_afq(&self) -> f64 {
        self.postflop_raise_rate()
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

    pub fn river_overbet_ratio(&self) -> f64 {
        let river_bets = self.river_actions[2].max(self.river_overbets);
        if river_bets == 0 {
            0.0
        } else {
            self.river_overbets as f64 / river_bets as f64
        }
    }

    pub fn is_overbet_bluffer(&self) -> bool {
        if !self.exploit_enabled {
            return false;
        }
        /* Value-heavy showdowns veto bluff classification for polarized value bettors */
        if self.showdown_large_monster_count >= 2
            && self.showdown_large_monster_count > self.showdown_bluff_count * 2
        {
            return false;
        }
        /* Passive or balanced opponents who merely use occasional 150% value sizes are not spammers */
        if self.total_actions() >= 12 && self.raise_ratio() < 0.30 {
            return false;
        }
        let pf_ob_ratio = self.postflop_overbet_ratio();
        let min_ob_ratio = if self.raise_ratio() >= 0.45 {
            0.30
        } else {
            0.45
        };
        let high_postflop_overbet = self.postflop_bets_total >= 4
            && self.postflop_overbets >= 3
            && pf_ob_ratio >= min_ob_ratio;
        let high_river_overbet = self.postflop_bets_total >= 4
            && self.river_overbets >= 2
            && self.river_overbet_ratio() >= 0.45
            && (self.total_hands <= 20 || pf_ob_ratio >= 0.25 || self.raise_ratio() >= 0.40);
        let early_overbet_burst = (2..=3).contains(&self.postflop_bets_total)
            && self.postflop_overbets >= 2
            && pf_ob_ratio >= 0.65;
        high_postflop_overbet || high_river_overbet || early_overbet_burst
    }

    /** Opponent bluff-density multiplier passed to `SubgameSolver` reach likelihoods. */
    pub fn subgame_bluff_multiplier(&self) -> Option<f64> {
        if !self.exploit_enabled {
            None
        } else if self.is_overbet_bluffer() {
            let confirmed_maniac = self.smoothed_raise_ratio() >= 0.45
                || (self.showdown_hands >= 2 && self.showdown_bluff_rate() >= 0.35);
            let raw_mult = if confirmed_maniac {
                (1.5 + self.postflop_overbet_ratio() * 1.3).clamp(1.7, 2.6)
            } else {
                (1.2 + self.postflop_overbet_ratio() * 0.7).clamp(1.3, 1.8)
            };
            if self.showdown_hands == 0 {
                Some(raw_mult.min(1.65))
            } else {
                Some(raw_mult)
            }
        } else if self.total_actions() >= 12
            && self.smoothed_raise_ratio() > 0.40
            && self.showdown_large_monster_count <= self.showdown_bluff_count + 1
        {
            Some((1.2 + (self.smoothed_raise_ratio() - 0.35) * 3.0).clamp(1.2, 2.1))
        } else if self.showdown_hands >= 3
            && self.showdown_bluff_rate() > 0.40
            && self.showdown_large_monster_count <= self.showdown_bluff_count
        {
            Some(1.6)
        } else if self.total_actions() >= 12
            && self.smoothed_vpip() < 0.28
            && self.smoothed_raise_ratio() < 0.20
        {
            Some(0.55)
        } else {
            None
        }
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

    pub fn flop_cbet_freq(&self) -> f64 {
        if self.flop_cbet_opps == 0 {
            0.55
        } else {
            self.flop_cbet_made as f64 / self.flop_cbet_opps as f64
        }
    }

    pub fn turn_barrel_freq(&self) -> f64 {
        if self.turn_barrel_opps == 0 {
            0.45
        } else {
            self.turn_barrel_made as f64 / self.turn_barrel_opps as f64
        }
    }

    pub fn overbet_value_rate(&self) -> f64 {
        if self.showdown_overbet_hands == 0 {
            0.50
        } else {
            self.showdown_overbet_value_hands as f64 / self.showdown_overbet_hands as f64
        }
    }

    pub fn postflop_raise_rate(&self) -> f64 {
        let post_total: u64 = self.flop_actions.iter().sum::<u64>()
            + self.turn_actions.iter().sum::<u64>()
            + self.river_actions.iter().sum::<u64>();
        if post_total == 0 {
            0.22
        } else {
            let post_raises = self.flop_actions[2] + self.turn_actions[2] + self.river_actions[2];
            post_raises as f64 / post_total as f64
        }
    }

    /** Measured street-level opponent behavioral frequencies for `SubgameSolver` node-locking. */
    pub fn subgame_street_profile(&self) -> Option<crate::cfr::subgame::OppStreetProfile> {
        if !self.exploit_enabled {
            return None;
        }
        let post_total: u64 = self.flop_actions.iter().sum::<u64>()
            + self.turn_actions.iter().sum::<u64>()
            + self.river_actions.iter().sum::<u64>();
        let flop_fold_to_cbet =
            (self.flop_cbet_faced >= 4).then(|| self.smoothed_flop_fold_to_cbet());
        let turn_fold_to_barrel =
            (self.turn_cbet_faced >= 3).then(|| self.smoothed_turn_fold_to_cbet());
        let river_fold_to_bet =
            (self.river_bet_faced >= 3).then(|| self.smoothed_river_fold_to_bet());
        let postflop_raise_rate = (post_total >= 8).then(|| self.postflop_raise_rate());
        let overbet_freq = (self.postflop_bets_total >= 3).then(|| self.postflop_overbet_ratio());
        let overbet_value_rate =
            (self.showdown_overbet_hands >= 2).then(|| self.overbet_value_rate());

        let prof = crate::cfr::subgame::OppStreetProfile {
            flop_fold_to_cbet,
            turn_fold_to_barrel,
            river_fold_to_bet,
            postflop_raise_rate,
            overbet_freq,
            overbet_value_rate,
        };
        if prof.flop_fold_to_cbet.is_some()
            || prof.turn_fold_to_barrel.is_some()
            || prof.river_fold_to_bet.is_some()
            || prof.postflop_raise_rate.is_some()
            || prof.overbet_freq.is_some()
            || prof.overbet_value_rate.is_some()
        {
            Some(prof)
        } else {
            None
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
