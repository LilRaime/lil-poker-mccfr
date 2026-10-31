/* Exploitative Strategy Adjustment and Purified Action Sampling */
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::{
    detect_board_texture, detect_river_nut_blocker, is_dangerous_board, is_four_flush_board,
    is_four_straight_board, preflop_strength_tier,
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
            /* On the River, one-pair and overpair hands (28..=56) are bluff-catchers/thin-value, NOT monster hands!
             * Only genuine Two Pair+ (bucket >= 57) should be treated as strong for overbets/shoves. */
            bucket >= 57
        } else {
            bucket >= 40
        };
        let is_weak = if is_preflop {
            pf_tier >= 7
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
            if f3b > 0.60 && (2..=4).contains(&pf_tier) {
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
                    if round == 4 && bucket == 73 {
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
         * When opponent has high SB limp frequency (> 20%) and we are on BB with playable hand (pf_tier <= 5 && bucket <= 105):
         * Heavily punish limpers with ISO-raises to capture dead money. Never ISO-raise weak offsuit trash like J3o!
         */
        if is_preflop && self.sb_limp_ratio() > 0.20 && pf_tier <= 5 && bucket <= 105 {
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
         * or if we hold a postflop Monster/Nut hand (bucket >= 83, or 2nd-Nut Flush bucket == 82 on non-4-flush
         * boards when not facing a >= 2x pot overbet), never fold!
         * Also bridge to ALL_IN if CALL_CHECK is absent from server legal_actions. */
        let is_four_flush_board_now = board.map(is_four_flush_board).unwrap_or(false);
        let never_fold_postflop = !is_preflop
            && (bucket >= 83 || (bucket == 82 && !is_four_flush_board_now && to_call < pot * 2));
        if to_call > 0
            && ((pot_odds <= 0.15 && (is_preflop || bucket >= 15)) || never_fold_postflop)
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

        /* Preflop Trash, Priced-In BB Defense & Limp-Pot OOP ISO-Raise Guard:
         * 1) In BB facing a limp (to_call == 0): never iso-raise OOP with Tier 6..=8 hands (e.g. 53s, J3o); take the free flop!
         * 2) Facing a bet with Tier 7..=8 hands (e.g. 54o, 72o): never 3x open-raise or 3-bet bluff!
         * 3) Priced-In BB Protection (Hands G2-1, G2-2, G2-3): if pot_odds <= 0.26 (e.g. BB already posted facing a min-raise
         *    or short-stack shove), do NOT force-fold playable/borderline hands! */
        if is_preflop {
            let pf_tier = preflop_strength_tier(bucket);
            if to_call == 0 && pf_tier >= 6 {
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
            } else if to_call > 0 && pf_tier >= 7 {
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if pot_odds <= 0.22 || (pot_odds <= 0.26 && pf_tier <= 7) {
                    /* Priced in from the Big Blind: shift removed raise weight into calling rather than folding */
                    if let Some(c_i) = legal_actions
                        .iter()
                        .position(|&a| a == CALL_CHECK)
                        .or_else(|| legal_actions.iter().position(|&a| a == ALL_IN))
                    {
                        probs[c_i] += shifted;
                    }
                } else if to_call > bb / 2 && (pf_tier == 8 || pot_odds > 0.28) {
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
         * On the River, require to_call to be either an actual overbet/heavy bet relative to the pot (pot_odds >= 0.30, >= 4 BB)
         * or an all-in shove (>= ALLIN_GUARD_THRESHOLD or !can_raise). */
        let is_substantial_bet = to_call >= bb * 4;
        let is_shove_faced = is_substantial_bet
            && pot_odds > 0.18
            && (to_call >= eff_allin_threshold
                || !can_raise
                || (round == 4 && to_call >= bb * 5 && pot_odds >= 0.30)
                || (round == 3 && to_call >= bb * 15 && pot_odds >= 0.33)
                || (round == 2 && to_call >= bb * 20));

        if is_shove_faced {
            let is_known_maniac = self.total_actions() >= 8
                && (self.raise_ratio() > 0.35
                    || self.pfr_ratio() > 0.40
                    || self.vpip_ratio() > 0.65);

            if is_preflop {
                let pf_tier = preflop_strength_tier(bucket);
                if to_call >= bb * 40 {
                    /* Facing massive cold shove (>= 40 BB, Match 3): AA and KK (bucket <= 1) never fold;
                     * against a known maniac, QQ and AK (pf_tier == 1) also never fold. */
                    if bucket <= 1 || (is_known_maniac && pf_tier == 1) {
                        if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                            probs[f_i] = 0.0;
                        }
                    }
                } else if pf_tier <= 1 && to_call < eff_allin_threshold * 2 {
                    /* Tier 1 (AA, KK, QQ, AK) never fold preflop to standard shoves */
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        probs[f_i] = 0.0;
                    }
                }
            }

            let is_trash = if is_preflop {
                let pf_tier = preflop_strength_tier(bucket);
                if pot_odds <= 0.22 || (pot_odds <= 0.26 && pf_tier <= 7) {
                    /* Priced in from BB against a short-stack shove (Hands G2-2, G2-3): never auto-fold! */
                    false
                } else if to_call >= bb * 40 {
                    /* Facing massive cold shove (>= 40 BB, Match 3):
                     * - vs known maniac: call with Tier 1 (AA, KK, QQ, AK)
                     * - vs tight/unknown in multi-way: only AA, KK (bucket <= 1)
                     * - vs tight/unknown in HU: AA, KK, QQ, AKs (bucket <= 2 || bucket == 13) */
                    if is_known_maniac {
                        pf_tier > 1
                    } else if active_opponents >= 2 {
                        bucket > 1
                    } else {
                        !(bucket <= 2 || bucket == 13)
                    }
                } else if to_call >= eff_allin_threshold {
                    /* Facing 35-39 BB shove:
                     * - vs known maniac: call with Tiers 1-2 (AA-99, AK, AQ, AJs, KQs)
                     * - vs tight/unknown in HU: Tier 1 + JJ, AQs (bucket == 3 || bucket == 14)
                     * - vs tight/unknown in multi-way: Tier 1 only */
                    if is_known_maniac {
                        pf_tier > 2
                    } else if active_opponents == 1 {
                        pf_tier > 1 && bucket != 3 && bucket != 14
                    } else {
                        pf_tier > 1
                    }
                } else {
                    /* Facing standard shove (< 35 BB): Tiers 5..=8 fold */
                    pf_tier >= 5
                }
            } else if round == 4 {
                /* On River facing massive overbet/shove: only High Overpair (54+) or Genuine Two Pair+ (57+) can call;
                 * on a 4-flush board, baby/board flushes (bucket <= 79) and non-flushes must also fold!
                 * on a 4-straight board, hands below Genuine Top Two Pair (bucket < 61) must fold! */
                let is_four_flush = board.map(is_four_flush_board).unwrap_or(false);
                let is_four_straight = board.map(is_four_straight_board).unwrap_or(false);
                bucket < 54 || (is_four_flush && bucket <= 79) || (is_four_straight && bucket < 61)
            } else {
                /* Flop/Turn facing massive shove:
                 * On Flop (2 cards to come), strong draws have 32-54% equity and can call <= 0.35 pot odds.
                 * On Turn (1 card to come), only Combo Draws (24..=27) with <= 0.31 pot odds or Nut FD (22..=23) with <= 0.22 can call! */
                let is_wet = board.map(is_dangerous_board).unwrap_or(false);
                let is_paired = board
                    .map(|b| detect_board_texture(b).is_paired)
                    .unwrap_or(false);
                let is_strong_draw = (15..=18).contains(&bucket) || (19..=27).contains(&bucket);
                let draw_can_call_shove = if round == 3 {
                    (bucket >= 24 && pot_odds <= 0.31 && !is_three_flush)
                        || ((bucket == 26 || bucket == 27) && pot_odds <= 0.31)
                        || ((bucket == 22 || bucket == 23) && pot_odds <= 0.22)
                } else {
                    (is_strong_draw && pot_odds <= 0.35 && !is_three_flush)
                        || ((bucket == 22 || bucket == 23 || bucket >= 25) && pot_odds <= 0.35)
                };

                if draw_can_call_shove {
                    false
                } else if is_three_flush && bucket <= 55 {
                    /* Bare one pair or overpair without a made flush on a 3-flush board facing a massive shove */
                    true
                } else if round == 3 && is_paired && bucket < 57 {
                    /* Turn on paired board facing massive shove: no Genuine Two Pair / Trips / Set / FH (bucket < 57, Match 7) is trash */
                    true
                } else if bucket < 46 && !is_strong_draw {
                    true
                } else if round == 3 && (46..=55).contains(&bucket) && is_wet {
                    /* Bare one pair or overpair on a 4-to-straight or monotone turn is trash against a massive shove */
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

        /* 1b. Postflop Bluff-Spew, Gutshot/Turn-Draw Overbet, and Turn/River Pot-Control Guard:
         * - Prevent suicidal all-in bluffs on ALL postflop streets (round >= 2) with hands below Trips/Set (bucket < 65),
         *   except Flop monster combo draws (25..=27).
         * - Suppress large overbets (RAISE_HALF_POT) on Turn/River with draws or air (bucket < 28) in medium/large pots (Match 10).
         * - On a 3-flush board without a flush, unmade draws (bucket < 28) must never lead-raise; take the free card!
         * - Turn facing a bet: never probe-raise with weak or marginal hands (bucket < 61, Match 7).
         * - Fold bare gutshots / pure air (bucket < 15) facing >= 33% pot bets (pot_odds >= 0.25, to_call >= 2 BB).
         * - Enforce strict single-card direct equity pot-odds on Turn draws (15..=27).
         * - Suppress turn/river lead donk-bets in bloated pots with multi-overcard underpairs/weak pairs (bucket 28..=38, Match 5). */
        if !is_preflop && bucket < 65 && !(round == 2 && (25..=27).contains(&bucket)) {
            /* All postflop streets: block all-in pushes for air, draws (except flop monster combo draws),
             * one-pair, overpair, and two-pair hands (< 65, Match 9 & Item I) */
            if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                probs[ai_i] = 0.0;
            }
        }

        if !is_preflop && (bucket < 15 || bucket == 28 || bucket == 29) {
            if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                probs[ai_i] = 0.0;
            }
            if to_call > 0 && (bucket < 15 || bucket == 28) {
                /* Facing a bet with pure air, gutshot, or crushed 4-flush/4-straight pair: never bluff-raise */
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        probs[i] = 0.0;
                    }
                }
                /* Strict Odds Guard (Match 4 & Item E): facing >= 33% pot bet (pot_odds >= 0.25, >= 2 BB) on Flop/Turn
                 * (or >= 16% pot odds on Turn where a gutshot has only 8.7% equity) with bucket < 15 or 28, force 100% fold! */
                let overpriced_gutshot_or_air =
                    (round < 4 && to_call >= bb * 2 && pot_odds >= 0.25)
                        || (round == 3 && to_call >= bb && pot_odds >= 0.16 && bucket < 15);
                if overpriced_gutshot_or_air {
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        for p in probs.iter_mut() {
                            *p = 0.0;
                        }
                        probs[f_i] = 1.0;
                    }
                }
            }
        }

        /* Turn Draw Direct Pot-Odds Guard (Item J):
         * On the Turn (round == 3), only 1 card remains: OESD (15..=18) has ~17.4% equity,
         * Flush Draw (19..=23) has ~19.6% equity, and Combo Draw (24..=27) has ~26-32% equity. */
        if round == 3 && to_call >= bb * 2 && (15..=27).contains(&bucket) {
            let max_turn_draw_odds = if bucket >= 24 {
                0.31
            } else if bucket >= 19 {
                0.22
            } else {
                0.19
            };
            if pot_odds > max_turn_draw_odds {
                if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                    for p in probs.iter_mut() {
                        *p = 0.0;
                    }
                    probs[f_i] = 1.0;
                }
            }
        }

        if round >= 3 && to_call == 0 && bucket < 28 {
            /* Large turn/river bluffs with bare draws / air:
             * On 3-flush boards or pots >= 6 BB, block RAISE_HALF_POT and shift all raises into CALL_CHECK (Match 10) */
            if let Some(hp_i) = legal_actions.iter().position(|&a| a == RAISE_HALF_POT) {
                probs[hp_i] = 0.0;
            }
            if pot >= bb * 6 || is_three_flush {
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
        } else if round >= 3 && to_call == 0 && pot >= bb * 8 && (28..=38).contains(&bucket) {
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

        /* Turn facing a bet: never probe-raise or check-raise with weak or marginal hands (bucket < 61, Match 7) */
        if round == 3 && to_call > 0 && bucket < 61 {
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    probs[i] = 0.0;
                }
            }
        }

        /* 2. Preflop Button Steal Exploitation (Item K):
         * In Heads-Up (active_opponents == 1), Button opens ~80% of hands profitably (tiers 1..=6).
         * Shifts both FOLD and passive open-limp (CALL_CHECK) weight into RAISE_MIN / RAISE_THIRD_POT!
         */
        let max_steal_tier = if active_opponents == 1 {
            6
        } else if active_opponents == 2 {
            4
        } else {
            3
        };
        let steal_max_call = bb / 2;
        let pf_tier_now = preflop_strength_tier(bucket);
        if is_preflop && to_call <= steal_max_call && pf_tier_now <= max_steal_tier {
            let fold_idx = legal_actions.iter().position(|&a| a == FOLD);
            let call_idx = legal_actions.iter().position(|&a| a == CALL_CHECK);
            let raise_idx = legal_actions
                .iter()
                .position(|&a| a == RAISE_MIN || a == RAISE_THIRD_POT);
            if let Some(r_i) = raise_idx {
                if let Some(f_i) = fold_idx {
                    if probs[f_i] > 0.05 {
                        probs[r_i] += probs[f_i] * 0.85;
                        probs[f_i] *= 0.15;
                    }
                }
                if to_call > 0 {
                    if let Some(c_i) = call_idx {
                        let limp_shift = if pf_tier_now <= 4 { 0.75 } else { 0.55 };
                        let shifted = probs[c_i] * limp_shift;
                        probs[r_i] += shifted;
                        probs[c_i] -= shifted;
                    }
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
         * - If we have bucket 73 (Board Straight or Idiot-End straight on 4-to-straight board),
         *   NEVER blast a 125% pot overbet or shove into a checked pot!
         * - With a one-pair, overpair, or vulnerable two-pair hand on River (28 <= bucket < 61), suppress 125% pot overbets
         *   (`RAISE_HALF_POT` on river) and all-in shoves so we don't value-own ourselves for stacks.
         * - On a 3-flush or dangerous board on Turn/River, if we hold less than a Flush (bucket < 79)
         *   and are facing a substantial bet (pot_odds > 0.25), do not re-raise / jam over the top! */
        if round == 4 && bucket == 73 {
            if let Some(r_hp) = legal_actions.iter().position(|&a| a == RAISE_HALF_POT) {
                probs[r_hp] *= 0.10;
            }
            if let Some(r_ai) = legal_actions.iter().position(|&a| a == ALL_IN) {
                probs[r_ai] *= 0.05;
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = probs[c_i].max(0.50);
            }
        } else if round == 4 && (28..61).contains(&bucket) {
            let is_dangerous = board.map(is_dangerous_board).unwrap_or(false) || is_three_flush;
            if to_call > 0 {
                /* Facing a River bet with One Pair, Overpair, or weak Two Pair (bucket < 61, e.g. Match 2 with 86o on 5-8-4-3-4):
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
            } else if bucket < 55 {
                /* Even on a dry river, one-pair and low/mid overpair hands (bucket < 55) should bet small/medium, not 125% overbet */
                if let Some(r_hp) = legal_actions.iter().position(|&a| a == RAISE_HALF_POT) {
                    probs[r_hp] *= 0.15;
                }
                if let Some(r_ai) = legal_actions.iter().position(|&a| a == ALL_IN) {
                    probs[r_ai] *= 0.05;
                }
            }
        } else if round >= 3
            && (79..=81).contains(&bucket)
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
            && bucket < 79
            && bucket != 72
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

                if pf_tier >= 6 || (pf_tier == 5 && is_offsuit && active_opponents >= 2) {
                    /* Pure trash, steal-tier, or weak offsuit in multi-way pot (Hand G2-1: 6d 3s):
                     * never raise and never open-limp; fold to any bet unless priced in from BB (pot_odds <= 0.22)! */
                    let mut removed_raise = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a >= RAISE_MIN {
                            removed_raise += probs[i];
                            probs[i] = 0.0;
                        }
                    }
                    if to_call > 0 {
                        if pot_odds <= 0.22 && pf_tier <= 7 {
                            if let Some(c_i) = call_idx {
                                probs[c_i] += removed_raise;
                            }
                        } else if let Some(f_i) = fold_idx {
                            let call_w = call_idx.map(|ci| probs[ci]).unwrap_or(0.0);
                            if let Some(ci) = call_idx {
                                probs[ci] = 0.0;
                            }
                            probs[f_i] = (probs[f_i] + removed_raise + call_w).max(0.95);
                        }
                    } else if let Some(c_i) = call_idx {
                        probs[c_i] = (probs[c_i] + removed_raise).max(0.95);
                    }
                } else if pf_tier == 5 || (pf_tier == 4 && is_offsuit && active_opponents >= 3) {
                    /* Speculative Tier 5 (or offsuit Tier 4 in 4+ way) in multi-way: do not 3-bet bluff;
                     * shift removed raise weight into FOLD for offsuit hands so we don't open-limp offsuit hands! */
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
                    if to_call > bb * 3 && pot_odds > 0.25 {
                        /* Facing a large preflop raise multi-way with Tier 5 -> fold */
                        if let Some(f_i) = fold_idx {
                            if let Some(c_i) = call_idx {
                                shifted += probs[c_i] * 0.80;
                                probs[c_i] *= 0.20;
                            }
                            probs[f_i] = (probs[f_i] + shifted).max(0.85);
                        }
                    } else if is_offsuit && to_call >= bb && pot_odds > 0.25 {
                        /* Never open-limp offsuit speculative hands in multi-way: shift damped raises into fold */
                        if let Some(f_i) = fold_idx {
                            probs[f_i] += shifted;
                        }
                    } else if let Some(c_i) = call_idx {
                        probs[c_i] += shifted;
                    }
                } else if (pf_tier == 3 || pf_tier == 4) && to_call >= bb * 4 {
                    /* Tier 3-4 facing heavy multi-way action: suppress 4-bet shoves, fold offsuit hands */
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a >= RAISE_HALF_POT {
                            probs[i] = 0.0;
                        }
                    }
                    if is_offsuit && active_opponents >= 3 && pot_odds > 0.26 {
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
                } else if (30..55).contains(&bucket) {
                    /* 3) One-Pair & Low/Mid Overpair hands (30..55) in multi-way pots:
                     *    Pot control! Avoid bloating pots or stacking off with a single pair against multiple opponents. */
                    let mut shifted = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a == ALL_IN || (a == RAISE_HALF_POT && bucket < 50) {
                            shifted += probs[i];
                            probs[i] = 0.0;
                        } else if a >= RAISE_MIN && (to_call > 0 || bucket < 46) {
                            let d = probs[i] * 0.75;
                            probs[i] -= d;
                            shifted += d;
                        }
                    }
                    if to_call > 0
                        && ((bucket < 44 && pot_odds >= 0.18)
                            || (bucket < 50 && pot_odds >= 0.28 && active_opponents >= 3))
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

                /* 4) Multi-way 4-Flush Board & Cold-Shove Defense (all buckets, Match 6):
                 *    - On a 4-flush board against multiple opponents, anything weaker than 2nd-Nut Flush (bucket < 82)
                 *      must NEVER bet/raise, and must fold to any meaningful bet (>= 4 BB or pot_odds >= 0.18).
                 *    - Facing a massive multi-way shove (to_call >= 15 BB and pot_odds > 0.18), require at least
                 *      Trips/Set (bucket >= 65) on clean boards or Strong Flush/Full House (bucket >= 80) on 3-flush boards! */
                let is_four_flush = board.map(is_four_flush_board).unwrap_or(false);
                if is_four_flush && bucket < 82 {
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
                        bucket < 65 || (is_three_flush && bucket < 80)
                    } else {
                        bucket < 57
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
