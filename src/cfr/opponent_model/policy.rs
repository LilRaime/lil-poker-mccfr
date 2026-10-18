/* Exploitative Strategy Adjustment and Purified Action Sampling */
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::detect_river_nut_blocker;
use crate::game::config::*;
use crate::game::holdem::Card;
use rand::Rng;

impl OpponentTracker {
    /* Blends Blueprint action probabilities with an exploitative adjustment. */
    pub fn adjust_strategy(&self, blueprint_probs: &[f64], legal_actions: &[u8]) -> Vec<f64> {
        self.adjust_strategy_full(blueprint_probs, legal_actions, 25, 2)
    }

    /* Advanced exploitative adjustment that knows whether our hand is strong, medium, or weak */
    pub fn adjust_strategy_with_context(
        &self,
        blueprint_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        is_preflop: bool,
    ) -> Vec<f64> {
        self.adjust_strategy_full(
            blueprint_probs,
            legal_actions,
            bucket,
            if is_preflop { 1 } else { 2 },
        )
    }

    /* Full round-aware exploitative adjustment incorporating Wilson confidence and multi-street barrels */
    pub fn adjust_strategy_full(
        &self,
        blueprint_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
    ) -> Vec<f64> {
        if !self.exploit_enabled || self.exploit_factor <= 1e-6 {
            return blueprint_probs.to_vec();
        }

        let n = blueprint_probs.len();
        let total_acts = self.total_actions();
        if total_acts < 6 {
            return blueprint_probs.to_vec();
        }

        let is_preflop = round == 1;

        /* Bayesian confidence weight: smooth transition from GTO to exploitation */
        let raw_conf = (total_acts as f64 - 5.0) / (total_acts as f64 + 20.0);
        let confidence = (raw_conf * self.exploit_factor).clamp(0.0, 0.75);

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
                    if a == CALL_CHECK {
                        adjusted[a as usize] *= 1.25;
                    } else if a == FOLD {
                        adjusted[a as usize] *= 0.75;
                    }
                }
            } else if bluff_rate < 0.10 && self.showdown_hands >= 4 {
                /* Opponent rarely bluffs: fold bluff catchers to big bets */
                if is_weak {
                    for &a in legal_actions {
                        if a == FOLD {
                            adjusted[a as usize] *= 1.35;
                        }
                    }
                }
            }
        }

        /* Showdown sizing tell inference: large bet monster bias */
        if self.showdown_large_monster_count >= 2 {
            let large_bets = self.sizing_large_bets.max(1) as f64;
            if self.showdown_large_monster_count as f64 / large_bets > 0.65 && is_weak {
                /* Opponent almost exclusively bets large with monsters: fold light */
                for &a in legal_actions {
                    if a == FOLD {
                        adjusted[a as usize] *= 1.30;
                    } else if a == CALL_CHECK {
                        adjusted[a as usize] *= 0.70;
                    }
                }
            }
        }

        /* Positional 3-bet exploit: If opponent folds to 3-bets heavily (> 60%), 3-bet bluff them with medium hands */
        if is_preflop && self.faced_3bet_count >= 3 {
            let f3b = self.fold_to_3bet_ratio();
            if f3b > 0.60 && (36..=85).contains(&bucket) {
                for &a in legal_actions {
                    if a == RAISE_THIRD_POT || a == RAISE_MIN {
                        adjusted[a as usize] *= 1.40;
                    }
                }
            } else if f3b < 0.25 && is_weak {
                /* Never folds to 3-bet: stop 3-bet bluffing */
                for &a in legal_actions {
                    if a >= RAISE_MIN {
                        adjusted[a as usize] *= 0.50;
                    }
                }
            }
        }

        /* Statistical confidence check using Wilson score interval (80% confidence level z = 1.28) */
        let (f_low, f_high) = Self::wilson_confidence(self.fold_count, total_acts, 1.28);

        if f_low > 0.42 || fold_rate > 0.45 {
            /* Opponent over-folds (Nit/Rock):
             * 1. Bluff cheaply with small sizing (RAISE_MIN, RAISE_THIRD_POT) where fold equity is highest per chip risked.
             * 2. Avoid costly large-bet bluffs (RAISE_HALF_POT, ALL_IN) which risk too many chips unnecessarily.
             * 3. With strong hands, bet small or smooth call to avoid scaring them out of the pot.
             */
            for &a in legal_actions {
                let idx = a as usize;
                if is_weak {
                    if a == RAISE_MIN {
                        adjusted[idx] *= 2.00;
                    } else if a == RAISE_THIRD_POT {
                        adjusted[idx] *= 2.20;
                    } else if a == RAISE_HALF_POT || a == ALL_IN {
                        adjusted[idx] *= 0.45;
                    } else if a == FOLD {
                        adjusted[idx] *= 0.50;
                    }
                } else if is_strong {
                    if a == RAISE_MIN || a == RAISE_THIRD_POT {
                        adjusted[idx] *= 1.45;
                    } else if a == RAISE_HALF_POT || a == ALL_IN {
                        adjusted[idx] *= 0.80;
                    } else if a == CALL_CHECK {
                        adjusted[idx] *= 1.25;
                    }
                } else {
                    /* Medium hands: steal aggressively with cheap raises */
                    if a == RAISE_MIN || a == RAISE_THIRD_POT {
                        adjusted[idx] *= 1.35;
                    } else if a == FOLD {
                        adjusted[idx] *= 0.70;
                    }
                }
            }
        } else if (f_high < 0.25 && call_rate > 0.40)
            || (fold_rate < 0.22 && call_rate > 0.40)
            || self.vpip_ratio() > 0.70
        {
            /* Calling station opponent:
             * 1. Extract MAX VALUE with massive sizing (RAISE_HALF_POT, ALL_IN) since stations call any size.
             * 2. Small min-raises waste massive EV against inelastic callers.
             * 3. NEVER bluff weak hands (reduce raises to near zero).
             * 4. Never fold value hands.
             */
            for &a in legal_actions {
                let idx = a as usize;
                if is_strong {
                    if a == RAISE_HALF_POT {
                        adjusted[idx] *= 2.50;
                    } else if a == ALL_IN {
                        adjusted[idx] *= 2.30;
                    } else if a == RAISE_THIRD_POT {
                        adjusted[idx] *= 1.65;
                    } else if a == RAISE_MIN {
                        adjusted[idx] *= 1.15;
                    } else if a == CALL_CHECK {
                        adjusted[idx] *= 0.50;
                    } else if a == FOLD {
                        adjusted[idx] *= 0.02;
                    }
                } else if is_weak {
                    if a >= RAISE_MIN {
                        adjusted[idx] *= 0.03; /* Stop bluffing! */
                    } else if a == FOLD {
                        adjusted[idx] *= 1.45;
                    }
                } else {
                    /* Medium hands: thin value bet with small-to-medium sizing */
                    if a == RAISE_MIN || a == RAISE_THIRD_POT {
                        adjusted[idx] *= 1.30;
                    }
                }
            }
        } else if raise_rate > 0.40 || self.pfr_ratio() > 0.45 {
            /* Hyper-aggressive / Maniac:
             * 1. Trap with strong hands (allow them to barrel, smooth call).
             * 2. Call down with bluff-catchers (especially on Turn/River where they over-barrel).
             * 3. Ditch trash cleanly without paying off their bets.
             */
            for &a in legal_actions {
                let idx = a as usize;
                if is_strong {
                    if a == CALL_CHECK {
                        adjusted[idx] *= 1.85; /* Trap and induce barrels */
                    } else if a >= RAISE_MIN {
                        adjusted[idx] *= 0.70;
                    }
                } else if is_weak {
                    if a == FOLD {
                        adjusted[idx] *= 1.40;
                    } else if a >= RAISE_MIN {
                        adjusted[idx] *= 0.05; /* Don't bluff into maniacs */
                    }
                } else {
                    /* Medium / bluff-catcher hand */
                    if round >= 3 && a == CALL_CHECK {
                        adjusted[idx] *= 1.50; /* Bluff-catch turn/river */
                    } else if a == FOLD {
                        adjusted[idx] *= 0.40; /* Don't fold bluff-catchers */
                    }
                }
            }
        }

        /* Situational C-bet and Barrel fold exploits across streets */
        if round == 2 && is_weak && self.flop_fold_to_cbet_ratio() > 0.55 {
            for &a in legal_actions {
                if a == RAISE_THIRD_POT || a == RAISE_HALF_POT {
                    adjusted[a as usize] *= 1.45; /* Fire continuation bet bluff on flop! */
                }
            }
        } else if round == 3 && is_weak && self.turn_fold_to_cbet_ratio() > 0.55 {
            for &a in legal_actions {
                if a == RAISE_THIRD_POT || a == RAISE_HALF_POT {
                    adjusted[a as usize] *= 1.50; /* Fire 2nd barrel bluff on turn! */
                }
            }
        } else if round == 4 && is_weak && self.river_fold_to_bet_ratio() > 0.55 {
            for &a in legal_actions {
                if a == RAISE_THIRD_POT || a == RAISE_HALF_POT {
                    adjusted[a as usize] *= 1.55; /* Fire river probe bluff! */
                }
            }
        }

        /* Preflop Limp-Punishing (ISO-Raise):
         * When opponent has high SB limp frequency (> 20%) and we are on BB with playable hand (bucket <= 110):
         * Heavily punish limpers with ISO-raises to capture dead money.
         */
        if is_preflop && self.sb_limp_ratio() > 0.20 && bucket <= 110 {
            for &a in legal_actions {
                if a == RAISE_MIN || a == RAISE_THIRD_POT {
                    adjusted[a as usize] *= 1.65;
                } else if a == CALL_CHECK {
                    adjusted[a as usize] *= 0.40;
                }
            }
        }

        /* Float / Delay C-bet on missed opponent c-bets:
         * If opponent checks on the flop (> 50% flop check rate), their range on turn/river is capped.
         * Attack with probe bets on the Turn (round == 3).
         */
        if round == 3 && self.flop_check_ratio() > 0.50 {
            for &a in legal_actions {
                if a == RAISE_THIRD_POT || a == RAISE_HALF_POT {
                    adjusted[a as usize] *= 1.45;
                }
            }
        }

        /* Apply Safety Guardrails: Prevent catastrophic over-exploitation and keep defense balanced */
        for &a in legal_actions {
            let idx = a as usize;
            if idx < n {
                let bp = blueprint_probs[idx];
                if bp > 0.005 {
                    let min_bound = 0.02 * bp;
                    let max_bound = 2.80 * bp;
                    adjusted[idx] = adjusted[idx].clamp(min_bound, max_bound);
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

    /* Purified and noise-filtered action sampling with all-in defense (backward-compatible) */
    pub fn select_action_purified<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        is_preflop: bool,
        to_call: i32,
        rng: &mut R,
    ) -> u8 {
        self.select_action_purified_round(
            raw_probs,
            legal_actions,
            bucket,
            if is_preflop { 1 } else { 2 },
            to_call,
            rng,
        )
    }

    /* Street-aware purified action sampling */
    pub fn select_action_purified_round<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        rng: &mut R,
    ) -> u8 {
        self.select_action_purified_context(
            raw_probs,
            legal_actions,
            bucket,
            round,
            to_call,
            None,
            None,
            rng,
        )
    }

    /* Full contextual purified action sampling with hole and board awareness for blocker effects */
    #[allow(clippy::too_many_arguments)]
    pub fn select_action_purified_context<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        rng: &mut R,
    ) -> u8 {
        let is_preflop = round == 1;
        let adjusted_probs = self.adjust_strategy_full(raw_probs, legal_actions, bucket, round);

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
                if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
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
            } else if round == 4 {
                /* On River facing massive overbet/shove (35+ BB): only Two Pair+ (>= 48) or strong TPTK can call */
                bucket < 48
            } else {
                /* Flop/Turn facing massive shove: need strong made hand (>= 38) or monster combo draw (>= 24) */
                bucket < 38 && bucket != 24 && bucket != 25
            };

            if is_trash {
                let has_fold = legal_actions.contains(&FOLD);
                if has_fold {
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a == FOLD {
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
            let fold_idx = legal_actions.iter().position(|&a| a == FOLD);
            let raise_idx = legal_actions
                .iter()
                .position(|&a| a == RAISE_MIN || a == RAISE_THIRD_POT);
            if let (Some(f_i), Some(r_i)) = (fold_idx, raise_idx) {
                if probs[f_i] > 0.05 {
                    probs[r_i] += probs[f_i] * 0.85;
                    probs[f_i] *= 0.15;
                }
            }
        }

        /* 3. Blocker-aware River Bluffing:
         * On the River (round == 4) with weak hands / bluff-catchers (bucket < 28):
         * If we hold the Nut Blocker (Ace of flush suit or Broadway straight blocker),
         * boost aggressive bluffing on RAISE_HALF_POT (125% overbet) or RAISE_THIRD_POT.
         */
        if round == 4 && bucket < 28 {
            if let (Some(h), Some(b)) = (hole, board) {
                if detect_river_nut_blocker(h, b) {
                    let raise_overbet = legal_actions.iter().position(|&a| a == RAISE_HALF_POT);
                    let raise_third = legal_actions.iter().position(|&a| a == RAISE_THIRD_POT);
                    let check_idx = legal_actions.iter().position(|&a| a == CALL_CHECK);
                    let fold_idx = legal_actions.iter().position(|&a| a == FOLD);

                    if let Some(r_i) = raise_overbet.or(raise_third) {
                        probs[r_i] = (probs[r_i] * 1.50).max(0.20);
                        if let Some(c_i) = check_idx {
                            probs[c_i] *= 0.65;
                        }
                        if let Some(f_i) = fold_idx {
                            probs[f_i] *= 0.50;
                        }
                    }
                }
            }
        }

        /* 4. Noise Clamping:
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

        /* 5. Mild Temperature Sharpening (T = 0.85 => power 1.18):
         * Gently cleans up low-probability actions without distorting GTO balance.
         */
        let temp_inv = 1.18;
        for p in probs.iter_mut() {
            if *p > 0.0 {
                *p = p.powf(temp_inv);
            }
        }

        /* 6. Normalization and Sampling */
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
