/* Exploitative Strategy Adjustment and Purified Action Sampling */
use super::tracker::OpponentTracker;
use crate::game::config::{ALLIN_GUARD_THRESHOLD, PREFLOP_STEAL_MAX_TO_CALL};
use rand::Rng;

impl OpponentTracker {
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
    pub fn select_action_purified<R: Rng>(
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
