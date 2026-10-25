/* Exploitative Strategy Adjustment and Purified Action Sampling */
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::{
    detect_board_texture, detect_river_nut_blocker, is_dangerous_board, is_four_flush_board,
    preflop_strength_tier,
};
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

        let pf_tier = preflop_strength_tier(bucket);
        let is_strong = if is_preflop {
            pf_tier <= 2
        } else if round == 4 {
            /* On the River, one-pair hands (36..48) are bluff-catchers/thin-value, NOT monster hands!
             * Only genuine Two Pair+ (bucket >= 50) should be treated as strong for overbets/shoves. */
            bucket >= 50
        } else {
            bucket >= 36
        };
        let is_weak = if is_preflop {
            pf_tier >= 5
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
            if f3b > 0.60 && (2..=3).contains(&pf_tier) {
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
                    if round == 4 && bucket == 62 {
                        /* Board straight / idiot end on River is chop/bluff-catch, do not blast overbets */
                        if a == CALL_CHECK {
                            adjusted[idx] *= 1.80;
                        } else if a == RAISE_HALF_POT || a == ALL_IN {
                            adjusted[idx] *= 0.10;
                        }
                    } else if a == RAISE_HALF_POT {
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
                if a == RAISE_MIN || a == RAISE_THIRD_POT {
                    adjusted[a as usize] *= 1.55; /* Fire economical river probe bluff! */
                }
            }
        }

        /* Preflop Limp-Punishing (ISO-Raise):
         * When opponent has high SB limp frequency (> 20%) and we are on BB with playable hand (pf_tier <= 4 && bucket <= 105):
         * Heavily punish limpers with ISO-raises to capture dead money. Never ISO-raise weak offsuit trash like J3o!
         */
        if is_preflop && self.sb_limp_ratio() > 0.20 && pf_tier <= 4 && bucket <= 105 {
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
            to_call.max(BIG_BLIND) * 2,
            None,
            None,
            rng,
        )
    }

    /* Full contextual purified action sampling with hole and board awareness for blocker effects and pot odds */
    #[allow(clippy::too_many_arguments)]
    pub fn select_action_purified_context<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        pot: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        rng: &mut R,
    ) -> u8 {
        self.select_action_purified_context_multiway(
            raw_probs,
            legal_actions,
            bucket,
            round,
            to_call,
            pot,
            hole,
            board,
            1,
            rng,
        )
    }

    /* Multi-way aware purified action sampling (adapts ranges, bluff frequencies, and pot-control for 2..=6 players) */
    #[allow(clippy::too_many_arguments)]
    pub fn select_action_purified_context_multiway<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        pot: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        active_opponents: usize,
        rng: &mut R,
    ) -> u8 {
        self.select_action_purified_context_full(
            raw_probs,
            legal_actions,
            bucket,
            round,
            to_call,
            pot,
            hole,
            board,
            active_opponents,
            BIG_BLIND,
            rng,
        )
    }

    /* Blind-level and Multi-way aware purified action sampling */
    #[allow(clippy::too_many_arguments)]
    pub fn select_action_purified_context_full<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        pot: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        active_opponents: usize,
        table_bb: i32,
        rng: &mut R,
    ) -> u8 {
        let bb = if table_bb > 0 { table_bb } else { BIG_BLIND };
        let eff_allin_threshold = (bb * 35).max(ALLIN_GUARD_THRESHOLD);
        let active_opponents = active_opponents.max(1);
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

        /* 1. Pot-Odds Aware All-In Defense and Heavy Bet Guard:
         * Never call off stack with trash hands to massive bets due to mixed-strategy noise!
         * Conversely:
         * - If to_call is tiny or pot odds are overwhelming (pot_odds <= 0.15), NEVER fold (pot-committed)!
         * - Strong draws (OESD, Flush Draws, Combo Draws) on Flop/Turn have direct equity and must not fold to reasonable bets.
         * - Monster hands (Tier 1 & Tier 2: AA-99, AK, AQ) must never fold preflop to normal shoves!
         */
        let can_raise = legal_actions
            .iter()
            .any(|&a| (RAISE_MIN..=ALL_IN).contains(&a));

        let total_pot = pot + to_call;
        let pot_odds = if total_pot > 0 {
            to_call as f64 / total_pot as f64
        } else {
            1.0
        };

        /* If pot odds are dirt cheap (pot_odds <= 0.15) and we hold at least a draw or pair (or preflop),
         * or if we hold a postflop Monster/Nut hand (bucket >= 70: 2nd-Nut/Nut Flush, Full House, Quads, Straight Flush),
         * never fold! Also bridge to ALL_IN if CALL_CHECK is absent from server legal_actions. */
        if to_call > 0
            && ((pot_odds <= 0.15 && (is_preflop || bucket >= 15))
                || (!is_preflop && bucket >= 70))
        {
            if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                let f_prob = probs[f_i];
                probs[f_i] = 0.0;
                if let Some(c_i) = legal_actions
                    .iter()
                    .position(|&a| a == CALL_CHECK)
                    .or_else(|| legal_actions.iter().position(|&a| a == ALL_IN))
                {
                    probs[c_i] += f_prob.max(0.50);
                }
            }
        }

        /* Preflop Trash & Limp-Pot OOP ISO-Raise Guard:
         * 1) In BB facing a limp (to_call == 0): never iso-raise OOP with Tier 4 or Tier 5 hands (e.g. 53s, J3o); take the free flop!
         * 2) Facing any bet with Tier 5 offsuit trash (e.g. 54o, 72o): never 3x open-raise or 3-bet bluff! */
        if is_preflop {
            let pf_tier = preflop_strength_tier(bucket);
            if to_call == 0 && pf_tier >= 4 {
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] = (probs[c_i] + shifted).max(0.95);
                }
            } else if to_call > 0 && pf_tier >= 5 {
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if to_call > SMALL_BLIND {
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                            shifted += probs[c_i];
                            probs[c_i] = 0.0;
                        }
                        probs[f_i] = (probs[f_i] + shifted).max(0.98);
                    }
                } else if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                    probs[f_i] += shifted;
                }
            }
        }

        let is_three_flush = board
            .map(|b| {
                let mut sc = [0u8; 4];
                for c in b {
                    sc[c.suit as usize] += 1;
                }
                sc.iter().any(|&cnt| cnt >= 3)
            })
            .unwrap_or(false);

        /* A situation is a dangerous shove/heavy bet if to_call is substantial (>= 4 BB) and pot odds are meaningful (> 0.18).
         * On the River, require to_call to be either an actual overbet/heavy bet relative to the pot (pot_odds >= 0.35, i.e. >= 54% pot)
         * or an all-in shove (>= ALLIN_GUARD_THRESHOLD or !can_raise), so normal bets in big-blind escalated games don't auto-fold Two Pair. */
        let is_substantial_bet = to_call >= bb * 4;
        let is_shove_faced = is_substantial_bet
            && pot_odds > 0.18
            && (to_call >= eff_allin_threshold
                || !can_raise
                || (round == 4 && to_call >= bb * 10 && pot_odds >= 0.42)
                || (round == 3 && to_call >= bb * 15 && pot_odds >= 0.33)
                || (round == 2 && to_call >= bb * 20));

        if is_shove_faced {
            if is_preflop {
                let pf_tier = preflop_strength_tier(bucket);
                if pf_tier <= 2 && to_call < eff_allin_threshold * 2 {
                    /* Premium pocket pairs & monster hands (AA-99, AK, AQ) never fold preflop to normal shoves */
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        probs[f_i] = 0.0;
                    }
                }
            }

            let is_trash = if is_preflop {
                let pf_tier = preflop_strength_tier(bucket);
                if to_call >= eff_allin_threshold {
                    /* Facing massive 35+ BB shove/5-bet: only Tier 1 & 2 can call; Tier 3 (88, 77, 66) and below fold! */
                    pf_tier >= 3
                } else {
                    /* Facing standard shove: Tier 4 & 5 (small pairs 22-55, unsuited rags, weak suited) fold */
                    pf_tier >= 4
                }
            } else if round == 4 {
                /* On River facing massive overbet/shove: only genuine Two Pair+ (>= 48) can comfortably call;
                 * on a 4-flush board, baby/board flushes (bucket <= 67) and non-flushes must also fold! */
                let is_four_flush = board.map(is_four_flush_board).unwrap_or(false);
                bucket < 48 || (is_four_flush && bucket <= 67)
            } else {
                /* Flop/Turn facing massive shove:
                 * Strong draws (OESD, Flush Draws, Combo Draws) have 32-50% equity.
                 * With reasonable pot odds (<= 0.35), they are profitable calls and not trash! */
                let is_wet = board.map(is_dangerous_board).unwrap_or(false);
                let is_strong_draw = (15..=17).contains(&bucket) || (19..=27).contains(&bucket);
                if is_strong_draw && pot_odds <= 0.35 && !is_three_flush {
                    false
                } else if (bucket == 22 || bucket == 23 || bucket == 25) && pot_odds <= 0.35 {
                    /* Nut flush draw or monster combo draw can call reasonable pot odds even on 3-flush board */
                    false
                } else if is_three_flush && bucket <= 44 {
                    /* Bare one pair or overpair without a made flush on a 3-flush board facing a massive shove */
                    true
                } else if bucket < 38 && !is_strong_draw {
                    true
                } else if round == 3 && (38..=44).contains(&bucket) && is_wet {
                    /* Bare one pair or mid/low overpair on a 4-to-straight or monotone turn is trash against a massive shove */
                    true
                } else {
                    false
                }
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

        /* 1b. Postflop Bluff-Spew, Gutshot Overbet, and Underpair Pot-Control Guard:
         * - Prevent suicidal all-in bluffs and check-raise bluffs with pure air/board-pair.
         * - Fold bare gutshots / pure air (bucket < 15) facing large bets / overbets (pot_odds >= 0.35, to_call >= 4 BB).
         * - Suppress turn/river lead donk-bets in bloated pots with multi-overcard underpairs (bucket 28..=33, Match 5). */
        if !is_preflop && (bucket < 15 || bucket == 28 || bucket == 29) {
            if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                probs[ai_i] = 0.0;
            }
            if to_call > 0 && (bucket < 15 || bucket == 28) {
                /* Facing a bet with pure air or weak board-pair: never bluff-raise / check-raise */
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        probs[i] = 0.0;
                    }
                }
                /* Strict Odds Guard (Match 4): facing >= 54% pot bet (pot_odds >= 0.35, >= 4 BB) on Flop/Turn
                 * with a bare gutshot or air (bucket < 15), force 100% fold! */
                if round < 4 && to_call >= bb * 4 && pot_odds >= 0.35 {
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        for p in probs.iter_mut() {
                            *p = 0.0;
                        }
                        probs[f_i] = 1.0;
                    }
                }
            } else if round >= 3 && to_call == 0 && pot >= bb * 12 && bucket < 11 {
                /* In a large turn/river pot with pure air, suppress huge overbet/pot-sized bluffs */
                if let Some(hp_i) = legal_actions.iter().position(|&a| a == RAISE_HALF_POT) {
                    probs[hp_i] = 0.0;
                }
            }
        } else if round >= 3 && to_call == 0 && pot >= bb * 8 && (28..=33).contains(&bucket)
        {
            /* Underpair under 2+ overcards or weak middle/bottom pair in a large Turn/River pot (Match 5):
             * Check for pot control instead of turning showdown value into a burning lead-bluff! */
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        }

        /* Turn facing a bet: never probe-raise with weak or marginal hands (bucket < 45) */
        if round == 3 && to_call > 0 && bucket < 45 {
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    probs[i] = 0.0;
                }
            }
        }

        /* 2. Preflop Button Steal Exploitation:
         * In Heads-Up (active_opponents == 1), Button opens 80-85% of hands profitably (tiers 1-4).
         * In Multi-way (active_opponents >= 2), only auto-open strong hands (tiers 1-2, or tier 3 in 3-way).
         */
        let max_steal_tier = if active_opponents == 1 {
            4
        } else if active_opponents == 2 {
            3
        } else {
            2
        };
        let steal_max_call = bb / 2;
        if is_preflop
            && to_call <= steal_max_call
            && preflop_strength_tier(bucket) <= max_steal_tier
        {
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

        /* 3. Blocker-aware River Bluffing (Heads-Up only):
         * On the River (round == 4) when not facing a bet (to_call == 0) with weak hands (bucket < 28):
         * If we hold the Nut Blocker (Ace of flush suit or Broadway straight blocker),
         * boost aggressive bluffing on RAISE_HALF_POT (125% overbet) or RAISE_THIRD_POT.
         */
        if round == 4 && active_opponents == 1 && to_call == 0 && bucket < 28 {
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

        /* 4. Idiot-end / Board Straight and River Overbet safety:
         * - If we have bucket 62 (Board Straight or Idiot-End straight on 4-to-straight board),
         *   NEVER blast a 125% pot overbet or shove into a checked pot!
         * - With a one-pair or weak two-pair hand on River (28 <= bucket < 52), suppress 125% pot overbets
         *   (`RAISE_HALF_POT` on river) and all-in shoves so we don't value-own ourselves for stacks.
         * - On a 3-flush or dangerous board on Turn/River, if we hold less than a Flush (bucket < 67)
         *   and are facing a substantial bet (pot_odds > 0.25), do not re-raise / jam over the top! */
        if round == 4 && bucket == 62 {
            if let Some(r_hp) = legal_actions.iter().position(|&a| a == RAISE_HALF_POT) {
                probs[r_hp] *= 0.10;
            }
            if let Some(r_ai) = legal_actions.iter().position(|&a| a == ALL_IN) {
                probs[r_ai] *= 0.05;
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = probs[c_i].max(0.50);
            }
        } else if round == 4 && (28..52).contains(&bucket) {
            let is_dangerous = board.map(is_dangerous_board).unwrap_or(false) || is_three_flush;
            if to_call > 0 {
                /* Facing a River bet with One Pair or weak Two Pair (bucket < 52, e.g. Match 2 with 86o on 5-8-4-3-4):
                 * NEVER raise and pot-commit into a 3-bet shove! Shift all raise weight into flat calling. */
                let mut shifted_raise = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted_raise += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if shifted_raise > 0.0 {
                    if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                        probs[c_i] += shifted_raise;
                    }
                }
            } else if is_dangerous {
                /* On a 3-flush / 4-straight / coordinated paired river with one pair or weak two pair: never overbet or shove */
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a == RAISE_HALF_POT || a == ALL_IN {
                        probs[i] = 0.0;
                    }
                }
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] = probs[c_i].max(0.60);
                }
            } else if bucket < 46 {
                /* Even on a dry river, one-pair hands (bucket < 46) should bet small/medium, not 125% overbet */
                if let Some(r_hp) = legal_actions.iter().position(|&a| a == RAISE_HALF_POT) {
                    probs[r_hp] *= 0.15;
                }
                if let Some(r_ai) = legal_actions.iter().position(|&a| a == ALL_IN) {
                    probs[r_ai] *= 0.05;
                }
            }
        } else if round >= 3
            && (67..=68).contains(&bucket)
            && (board.map(is_four_flush_board).unwrap_or(false)
                || (is_three_flush
                    && board
                        .map(|b| detect_board_texture(b).is_paired)
                        .unwrap_or(false)))
        {
            /* Low/Mid flush (e.g. 5d 3d in Match 1 on 4-flush river or paired 3-flush turn):
             * Vulnerable to higher flushes and full houses -> suppress overbets, shoves, and raises facing a bet! */
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a == RAISE_HALF_POT || a == ALL_IN || (a >= RAISE_MIN && to_call > 0) {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] += shifted * 0.70;
            }
        }

        if !is_preflop
            && is_three_flush
            && to_call > 0
            && pot_odds >= 0.16
            && bucket < 67
            && bucket != 61
        {
            /* Facing a sizable bet on a 3-flush board without a made flush (and without Set+Flush redraw):
             * suppress re-raising / jamming (e.g. Hand 1 with bare trips A3o on 3-spade turn facing 150% overbet),
             * shifting raise probability into flat calling or folding. */
            let mut shifted_raise = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted_raise += probs[i];
                    probs[i] = 0.0;
                }
            }
            if shifted_raise > 0.0 {
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] += shifted_raise * 0.70;
                }
            }
        }

        /* 4b. Multi-Way Table Adaptation (3 to 6 players active, active_opponents >= 2):
         * Adjusts Heads-Up blueprint probabilities for multi-way equity compression,
         * exponential drop in multi-way fold equity, and tighter preflop selection. */
        if active_opponents >= 2 {
            let extra_opps = (active_opponents - 1).min(5) as i32;
            let fold_idx = legal_actions.iter().position(|&a| a == FOLD);
            let call_idx = legal_actions.iter().position(|&a| a == CALL_CHECK);

            if is_preflop {
                let pf_tier = preflop_strength_tier(bucket);
                let is_offsuit = bucket >= 91;

                if pf_tier >= 5 || (pf_tier == 4 && is_offsuit && active_opponents >= 2) {
                    /* Pure trash or weak offsuit in multi-way pot: never raise, fold to any bet */
                    let mut removed_raise = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a >= RAISE_MIN {
                            removed_raise += probs[i];
                            probs[i] = 0.0;
                        }
                    }
                    if to_call > 0 {
                        if let Some(f_i) = fold_idx {
                            let call_w = call_idx.map(|ci| probs[ci]).unwrap_or(0.0);
                            if let Some(ci) = call_idx {
                                probs[ci] = 0.0;
                            }
                            probs[f_i] = (probs[f_i] + removed_raise + call_w).max(0.95);
                        }
                    } else if let Some(c_i) = call_idx {
                        probs[c_i] = (probs[c_i] + removed_raise).max(0.95);
                    }
                } else if pf_tier == 4 {
                    /* Suited/connected Tier 4 hands in multi-way: do not 3-bet bluff; prefer cheap calls or folds */
                    let raise_damp = 0.40_f64.powi(extra_opps);
                    let mut shifted = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a >= RAISE_MIN {
                            if to_call > bb {
                                shifted += probs[i];
                                probs[i] = 0.0;
                            } else {
                                let d = probs[i] * (1.0 - raise_damp);
                                probs[i] -= d;
                                shifted += d;
                            }
                        }
                    }
                    if to_call > bb * 3 {
                        /* Facing a large preflop raise multi-way with Tier 4 -> fold */
                        if let Some(f_i) = fold_idx {
                            if let Some(c_i) = call_idx {
                                shifted += probs[c_i] * 0.80;
                                probs[c_i] *= 0.20;
                            }
                            probs[f_i] = (probs[f_i] + shifted).max(0.85);
                        }
                    } else if let Some(c_i) = call_idx {
                        probs[c_i] += shifted;
                    }
                } else if pf_tier == 3 && to_call >= bb * 4 {
                    /* Tier 3 facing heavy multi-way action: suppress 4-bet shoves, fold offsuit hands */
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a >= RAISE_HALF_POT {
                            probs[i] = 0.0;
                        }
                    }
                    if is_offsuit && active_opponents >= 3 {
                        if let Some(f_i) = fold_idx {
                            if let Some(c_i) = call_idx {
                                probs[f_i] += probs[c_i] * 0.70;
                                probs[c_i] *= 0.30;
                            }
                        }
                    }
                }
            } else {
                /* Postflop Multi-Way Adjustments (active_opponents >= 2):
                 * 1) Pure air or board-pair air (bucket < 15 || bucket == 28 || bucket == 29):
                 *    Fold equity across N opponents drops as p^N -> suppress multi-way bluffs! */
                if bucket < 15 || bucket == 28 || bucket == 29 {
                    let bluff_damp = 0.20_f64.powi(extra_opps);
                    let mut shifted = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a >= RAISE_HALF_POT || (a >= RAISE_MIN && to_call > 0) {
                            shifted += probs[i];
                            probs[i] = 0.0;
                        } else if a >= RAISE_MIN {
                            let d = probs[i] * (1.0 - bluff_damp);
                            probs[i] -= d;
                            shifted += d;
                        }
                    }
                    if to_call > 0 {
                        if let Some(f_i) = fold_idx {
                            if let Some(c_i) = call_idx {
                                shifted += probs[c_i];
                                probs[c_i] = 0.0;
                            }
                            probs[f_i] = (probs[f_i] + shifted).max(0.95);
                        }
                    } else if let Some(c_i) = call_idx {
                        probs[c_i] += shifted;
                    }
                } else if (15..=27).contains(&bucket) {
                    /* 2) Draws (15..=27): Great multi-way implied odds when calling cheap bets,
                     *    poor fold equity when semi-bluff raising into multiple players. */
                    let semibluff_damp = 0.35_f64.powi(extra_opps);
                    let mut shifted = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a == ALL_IN || (a >= RAISE_MIN && to_call > 0) {
                            shifted += probs[i];
                            probs[i] = 0.0;
                        } else if a >= RAISE_MIN {
                            let d = probs[i] * (1.0 - semibluff_damp);
                            probs[i] -= d;
                            shifted += d;
                        }
                    }
                    if to_call > 0 && pot_odds > 0.32 && bucket < 25 {
                        /* Non-nut draw facing expensive multi-way bet -> fold */
                        if let Some(f_i) = fold_idx {
                            probs[f_i] += shifted;
                        }
                    } else if let Some(c_i) = call_idx {
                        probs[c_i] += shifted;
                    }
                } else if (30..46).contains(&bucket) {
                    /* 3) One-Pair hands (30..46) in multi-way pots:
                     *    Pot control! Avoid bloating pots or stacking off with a single pair against multiple opponents. */
                    let mut shifted = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a == ALL_IN || (a == RAISE_HALF_POT && bucket < 42) {
                            shifted += probs[i];
                            probs[i] = 0.0;
                        } else if a >= RAISE_MIN && (to_call > 0 || bucket < 38) {
                            let d = probs[i] * 0.75;
                            probs[i] -= d;
                            shifted += d;
                        }
                    }
                    if to_call > 0
                        && ((bucket < 38 && pot_odds >= 0.18)
                            || (bucket < 42 && pot_odds >= 0.28 && active_opponents >= 3))
                    {
                        if let Some(f_i) = fold_idx {
                            if let Some(c_i) = call_idx {
                                let cd = probs[c_i] * 0.65;
                                probs[c_i] -= cd;
                                shifted += cd;
                            }
                            probs[f_i] += shifted;
                        }
                    } else if let Some(c_i) = call_idx {
                        probs[c_i] += shifted;
                    }
                }

                /* 4) Multi-way 4-Flush Board & Cold-Shove Defense (all buckets, including bucket >= 46, Match 6):
                 *    - On a 4-flush board against multiple opponents, anything weaker than 2nd-Nut Flush (bucket < 70)
                 *      must NEVER bet/raise, and must fold to any meaningful bet (>= 4 BB or pot_odds >= 0.18).
                 *    - Facing a massive multi-way shove (to_call >= 15 BB and pot_odds > 0.18), require at least
                 *      Trips/Set (bucket >= 55) on clean boards or Strong Flush/Full House (bucket >= 68) on 3-flush boards! */
                let is_four_flush = board.map(is_four_flush_board).unwrap_or(false);
                if is_four_flush && bucket < 70 {
                    let mut shifted = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a >= RAISE_MIN {
                            shifted += probs[i];
                            probs[i] = 0.0;
                        }
                    }
                    if to_call > 0 && (to_call >= bb * 4 || pot_odds >= 0.18) {
                        if let Some(f_i) = fold_idx {
                            for p in probs.iter_mut() {
                                *p = 0.0;
                            }
                            probs[f_i] = 1.0;
                        }
                    } else if let Some(c_i) = call_idx {
                        probs[c_i] = (probs[c_i] + shifted).max(0.95);
                    }
                } else if to_call >= bb * 15 && pot_odds > 0.18 {
                    let fold_multiway_shove = if round == 4 {
                        bucket < 55 || (is_three_flush && bucket < 68)
                    } else {
                        bucket < 48
                            && !(bucket == 22 || bucket == 23 || bucket == 25 || bucket == 27)
                    };
                    if fold_multiway_shove {
                        if let Some(f_i) = fold_idx {
                            for p in probs.iter_mut() {
                                *p = 0.0;
                            }
                            probs[f_i] = 1.0;
                        }
                    }
                }
            }
        }

        /* 5. Noise Clamping:
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
        } else if let Some(&c) = legal_actions.iter().find(|&&a| a == CALL_CHECK) {
            c
        } else {
            *legal_actions.first().unwrap_or(&1)
        }
    }
}
