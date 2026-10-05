/* Opponent Action Tracking and Exploitative Policy Generator. */
use crate::game::config::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpponentStyle {
    Unknown,
    CallingStation,
    Maniac,
    Rock,
    Tag,
    Lag,
}

impl std::fmt::Display for OpponentStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpponentStyle::Unknown => write!(f, "Unknown"),
            OpponentStyle::CallingStation => write!(f, "Calling Station (Loose-Passive)"),
            OpponentStyle::Maniac => write!(f, "Maniac (Hyper-Aggressive)"),
            OpponentStyle::Rock => write!(f, "Rock / Nit (Tight-Passive)"),
            OpponentStyle::Tag => write!(f, "TAG (Tight-Aggressive)"),
            OpponentStyle::Lag => write!(f, "LAG (Loose-Aggressive)"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpponentTracker {
    pub total_hands: u64,
    pub vpip_hands: u64,
    pub pfr_hands: u64,
    pub fold_count: u64,
    pub call_count: u64,
    pub raise_count: u64,

    /* Street-specific action counters [fold, call, raise] */
    pub preflop_actions: [u64; 3],
    pub flop_actions: [u64; 3],
    pub turn_actions: [u64; 3],
    pub river_actions: [u64; 3],

    /* Advanced situational metrics */
    pub flop_cbet_faced: u64,
    pub flop_fold_to_cbet: u64,
    pub went_to_showdown: u64,
    pub saw_flop: u64,

    /* Showdown observation analytics */
    pub showdown_hands: u64,
    pub showdown_bluff_count: u64,
    pub showdown_trap_count: u64,
    pub showdown_loose_count: u64,
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
            pfr_hands: 0,
            fold_count: 0,
            call_count: 0,
            raise_count: 0,
            preflop_actions: [0; 3],
            flop_actions: [0; 3],
            turn_actions: [0; 3],
            river_actions: [0; 3],
            flop_cbet_faced: 0,
            flop_fold_to_cbet: 0,
            went_to_showdown: 0,
            saw_flop: 0,
            showdown_hands: 0,
            showdown_bluff_count: 0,
            showdown_trap_count: 0,
            showdown_loose_count: 0,
        }
    }

    pub fn record_action(&mut self, action: u8, is_preflop: bool) {
        let round = if is_preflop { 1 } else { 2 };
        self.record_action_street(action, round, false);
    }

    pub fn record_action_street(&mut self, action: u8, round: u8, facing_cbet: bool) {
        let act_cat = match action {
            0 => 0,     /* Fold */
            1 => 1,     /* Call / Check */
            2..=5 => 2, /* Raise / Bet */
            _ => 1,
        };

        match round {
            1 => {
                self.preflop_actions[act_cat] += 1;
                if action == 1 {
                    self.vpip_hands += 1;
                } else if action >= 2 {
                    self.vpip_hands += 1;
                    self.pfr_hands += 1;
                }
            }
            2 => {
                self.saw_flop += 1;
                self.flop_actions[act_cat] += 1;
                if facing_cbet {
                    self.flop_cbet_faced += 1;
                    if action == 0 {
                        self.flop_fold_to_cbet += 1;
                    }
                }
            }
            3 => self.turn_actions[act_cat] += 1,
            4 => self.river_actions[act_cat] += 1,
            _ => {}
        }

        match action {
            0 => self.fold_count += 1,
            1 => self.call_count += 1,
            2..=5 => self.raise_count += 1,
            _ => {}
        }
    }

    pub fn end_hand(&mut self) {
        self.total_hands += 1;
    }

    pub fn record_showdown(&mut self) {
        self.went_to_showdown += 1;
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

    pub fn vpip_ratio(&self) -> f64 {
        if self.total_hands == 0 {
            0.50
        } else {
            self.vpip_hands as f64 / self.total_hands as f64
        }
    }

    pub fn pfr_ratio(&self) -> f64 {
        if self.total_hands == 0 {
            0.25
        } else {
            self.pfr_hands as f64 / self.total_hands as f64
        }
    }

    pub fn flop_fold_to_cbet_ratio(&self) -> f64 {
        if self.flop_cbet_faced == 0 {
            0.40
        } else {
            self.flop_fold_to_cbet as f64 / self.flop_cbet_faced as f64
        }
    }

    pub fn wtsd_ratio(&self) -> f64 {
        if self.saw_flop == 0 {
            0.30
        } else {
            self.went_to_showdown as f64 / self.saw_flop as f64
        }
    }

    /* Analyzes opponent's revealed cards at Showdown for deep range and tendency inference */
    pub fn record_showdown_hand(
        &mut self,
        opp_hole: [crate::game::holdem::Card; 2],
        board: &[crate::game::holdem::Card],
        opp_raised_river: bool,
    ) {
        self.showdown_hands += 1;
        self.went_to_showdown += 1;
        let score = crate::game::holdem::evaluate_7cards(&opp_hole, board);
        let cat = score >> 32;
        let (pf_idx, _) = crate::cfr::abstraction::preflop_bucket(opp_hole[0], opp_hole[1]);

        /* 1. Was opponent caught bluffing with weak hand / air on river? */
        if opp_raised_river && cat <= 1 {
            self.showdown_bluff_count += 1;
        }
        /* 2. Did opponent slowplay / trap with monster preflop hand? */
        if pf_idx <= 5 {
            self.showdown_trap_count += 1;
        }
        /* 3. Did opponent enter pot with trash cards? */
        if pf_idx > 90 {
            self.showdown_loose_count += 1;
        }
    }

    pub fn showdown_bluff_rate(&self) -> f64 {
        if self.showdown_hands == 0 {
            0.20
        } else {
            self.showdown_bluff_count as f64 / self.showdown_hands as f64
        }
    }

    /* Classifies opponent playstyle into a discrete archetype with confidence */
    pub fn classify_style(&self) -> (OpponentStyle, f64) {
        if self.total_hands < 3 {
            return (OpponentStyle::Unknown, 0.0);
        }
        let vpip = self.vpip_ratio();
        let pfr = self.pfr_ratio();
        let fold = self.fold_ratio();
        let raise = self.raise_ratio();

        let conf = (self.total_hands as f64 / (self.total_hands as f64 + 7.0)).min(0.95);

        if vpip > 0.60 && pfr < 0.20 && fold < 0.28 {
            (OpponentStyle::CallingStation, conf)
        } else if pfr > 0.38 && raise > 0.35 {
            (OpponentStyle::Maniac, conf)
        } else if vpip < 0.22 && fold > 0.45 {
            (OpponentStyle::Rock, conf)
        } else if (0.18..=0.35).contains(&vpip) && pfr >= 0.14 {
            (OpponentStyle::Tag, conf)
        } else if (0.35..=0.65).contains(&vpip) {
            (OpponentStyle::Lag, conf)
        } else {
            (OpponentStyle::CallingStation, conf * 0.7)
        }
    }

    /* Blends Blueprint action probabilities with an exploitative adjustment. */
    pub fn adjust_strategy(&self, blueprint_probs: &[f64], legal_actions: &[u8]) -> Vec<f64> {
        self.adjust_strategy_with_context(blueprint_probs, legal_actions, 25, false)
    }

    /* Advanced exploitative adjustment that knows whether our hand is strong, medium, or weak */
    pub fn adjust_strategy_with_context(
        &self,
        blueprint_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        is_preflop: bool,
    ) -> Vec<f64> {
        let n = blueprint_probs.len();
        let total_acts = self.total_actions();
        if total_acts < 3 {
            return blueprint_probs.to_vec();
        }

        /* Bayesian confidence weight: smooth transition from GTO to exploitation */
        let confidence = (total_acts as f64 / (total_acts as f64 + 10.0)).min(0.85);

        let mut adjusted = blueprint_probs.to_vec();
        let fold_rate = self.fold_ratio();
        let call_rate = self.call_ratio();
        let raise_rate = self.raise_ratio();

        let is_strong = if is_preflop {
            bucket <= 35
        } else {
            bucket >= 36
        };
        let is_weak = if is_preflop {
            bucket >= 95
        } else {
            bucket <= 14
        };

        /* Showdown bluff inference: adjust call-down frequency on bluff-catchers */
        if self.showdown_hands >= 2 {
            let bluff_rate = self.showdown_bluff_rate();
            if bluff_rate > 0.35 {
                /* Opponent caught bluffing often: call down lighter */
                for &a in legal_actions {
                    if a == 1 {
                        adjusted[a as usize] *= 1.25;
                    } else if a == 0 {
                        adjusted[a as usize] *= 0.75;
                    }
                }
            } else if bluff_rate < 0.10 && self.showdown_hands >= 4 {
                /* Opponent rarely bluffs: fold bluff catchers to big bets */
                if is_weak {
                    for &a in legal_actions {
                        if a == 0 {
                            adjusted[a as usize] *= 1.35;
                        }
                    }
                }
            }
        }

        if fold_rate > 0.45 {
            /* Opponent over-folds (Nit): Steal pots, bluff more, fold less */
            for &a in legal_actions {
                let idx = a as usize;
                if a >= 2 {
                    let mult = if is_weak { 1.60 } else { 1.30 };
                    adjusted[idx] *= mult;
                } else if a == 0 {
                    adjusted[idx] *= 0.55;
                }
            }
        } else if (fold_rate < 0.22 && call_rate > 0.40) || self.vpip_ratio() > 0.70 {
            /* Calling station opponent:
             * 1. Relentlessly value-bet strong hands (boost raises).
             * 2. NEVER bluff weak hands (reduce raises to near zero).
             * 3. Don't fold strong hands.
             */
            for &a in legal_actions {
                let idx = a as usize;
                if a >= 2 {
                    if is_strong {
                        adjusted[idx] *= 1.85;
                    } else if is_weak {
                        adjusted[idx] *= 0.05; /* Stop bluffing! */
                    }
                } else if a == 0 {
                    if is_strong {
                        adjusted[idx] *= 0.05;
                    } else if is_weak {
                        adjusted[idx] *= 1.35;
                    }
                } else if a == 1 && is_strong {
                    adjusted[idx] *= 0.75;
                }
            }
        } else if raise_rate > 0.40 || self.pfr_ratio() > 0.45 {
            /* Hyper-aggressive / Maniac:
             * 1. Trap with strong hands (allow them to barrel, smooth call).
             * 2. Call down with bluff-catchers (reduce folds on non-trash).
             * 3. Discard trash cleanly without paying off their bets.
             */
            for &a in legal_actions {
                let idx = a as usize;
                if a == 1 {
                    let mult = if is_strong { 1.65 } else { 1.30 };
                    adjusted[idx] *= mult;
                } else if a == 0 {
                    if !is_weak {
                        adjusted[idx] *= 0.40; /* Don't fold solid hands to aggressive bets */
                    } else {
                        adjusted[idx] *= 1.30; /* Quickly ditch trash */
                    }
                } else if a >= 2 && is_strong {
                    adjusted[idx] *= 0.85;
                }
            }
        }

        /* Situational C-bet fold exploit on flop */
        if !is_preflop && is_weak && self.flop_fold_to_cbet_ratio() > 0.55 {
            for &a in legal_actions {
                if a == 3 || a == 4 {
                    adjusted[a as usize] *= 1.45; /* Fire continuation bet bluff! */
                }
            }
        }

        /* Blend with Bayesian confidence */
        let mut blended = vec![0.0f64; n];
        for i in 0..n {
            blended[i] = (1.0 - confidence) * blueprint_probs[i] + confidence * adjusted[i];
        }

        /* Normalize strictly over legal actions */
        let mut sum = 0.0f64;
        for &a in legal_actions {
            let idx = a as usize;
            if idx < blended.len() {
                sum += blended[idx].max(0.0);
            }
        }

        if sum > 1e-9 {
            let inv_sum = 1.0 / sum;
            for p in blended.iter_mut() {
                *p = (*p * inv_sum).max(0.0);
            }
        } else {
            let u = 1.0 / legal_actions.len() as f64;
            blended = vec![0.0; n];
            for &a in legal_actions {
                blended[a as usize] = u;
            }
        }

        blended
    }

    /* Purified and noise-filtered action sampling with all-in defense */
    pub fn select_action_purified<R: rand::Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        is_preflop: bool,
        to_call: i32,
        rng: &mut R,
    ) -> u8 {
        let adjusted_probs =
            self.adjust_strategy_with_context(raw_probs, legal_actions, bucket, is_preflop);

        let mut probs: Vec<f64> = legal_actions
            .iter()
            .map(|&a| {
                let idx = a as usize;
                if idx < adjusted_probs.len() {
                    adjusted_probs[idx].max(0.0)
                } else {
                    0.0
                }
            })
            .collect();

        /* 1. Heavy Bet / All-In Defense Guard:
         * When facing a massive bet (to_call >= 350 chips),
         * never call off stack with trash/weak hands due to mixed-strategy noise!
         * Conversely, monster hands (bucket <= 12 preflop) must never fold to shoves!
         */
        if to_call >= ALLIN_GUARD_THRESHOLD {
            if is_preflop && bucket <= 12 {
                /* Premium pocket pairs never fold preflop to a shove */
                if let Some(f_i) = legal_actions.iter().position(|&a| a == 0) {
                    probs[f_i] = 0.0;
                }
            }

            let is_trash = if is_preflop {
                if bucket <= 12 {
                    false /* Pocket pairs (AA - 22) */
                } else if bucket <= 90 {
                    bucket > 75 /* Weakest suited hands */
                } else {
                    bucket > 120 /* Weakest offsuited hands (72o, 83o, etc.) */
                }
            } else {
                bucket < 24 /* Worse than One Pair or combo draw (complete air / missed draws) */
            };

            if is_trash {
                let has_fold = legal_actions.contains(&0);
                if has_fold {
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a == 0 {
                            probs[i] = 1.0;
                        } else {
                            probs[i] = 0.0;
                        }
                    }
                }
            }
        }

        /* 2. Preflop Button Steal Exploitation:
         * In Heads-Up, Button opens 80-85% of hands profitably.
         * For playable hands (bucket <= 120), shift fold probabilities into raises to steal blinds.
         */
        if is_preflop && to_call <= PREFLOP_STEAL_MAX_TO_CALL && bucket <= 120 {
            let fold_idx = legal_actions.iter().position(|&a| a == 0);
            let raise_idx = legal_actions.iter().position(|&a| a == 2 || a == 3);
            if let (Some(f_i), Some(r_i)) = (fold_idx, raise_idx) {
                if probs[f_i] > 0.05 {
                    probs[r_i] += probs[f_i] * 0.85;
                    probs[f_i] *= 0.15;
                }
            }
        }

        /* 3. Noise Clamping:
         * Suppress tiny exploration noise (< 2.5%) if a dominant legal move exists.
         */
        let max_prob = probs.iter().copied().fold(0.0f64, f64::max);
        if max_prob > 0.20 {
            for p in probs.iter_mut() {
                if *p < 0.025 {
                    *p = 0.0;
                }
            }
        }

        /* 3. Mild Temperature Sharpening (T = 0.85 => power 1.18):
         * Gently cleans up low-probability actions without distorting GTO balance.
         */
        let temp_inv = 1.18;
        for p in probs.iter_mut() {
            if *p > 0.0 {
                *p = p.powf(temp_inv);
            }
        }

        /* 4. Normalization and Sampling */
        let sum: f64 = probs.iter().sum();
        if sum > 1e-12 {
            let norm: Vec<f64> = probs.iter().map(|p| p / sum).collect();
            let r: f64 = rng.gen();
            let mut cum = 0.0;
            for (i, &p) in norm.iter().enumerate() {
                cum += p;
                if r < cum {
                    return legal_actions[i];
                }
            }
            *legal_actions.last().unwrap()
        } else {
            *legal_actions.first().unwrap_or(&1)
        }
    }
}
