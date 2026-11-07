/* Anti-Passive / Calling-Station Value-Extraction Boost & Postflop Barrel Defense */
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::{detect_board_texture, is_dangerous_board, preflop_strength_tier};
use crate::game::config::*;
use crate::game::holdem::Card;

impl OpponentTracker {
    /* Checks if opponent predominantly checks and calls without raising */
    pub fn is_passive_station(&self) -> bool {
        self.exploit_enabled
            && self.total_actions() >= 8
            && self.raise_ratio() < 0.12
            && (self.call_ratio() > 0.50 || self.fold_ratio() < 0.22)
    }

    /* Shift passive check/min-bet weight directly into geometric value sizing (RAISE_HALF_POT / RAISE_THIRD_POT)
     * so the bot extracts full stacks in 15-25 hands instead of checking down Top Pair / Overpair / Two Pair! */
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_passive_station_boost(
        &self,
        probs: &mut [f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        board: Option<&[Card]>,
        bb: i32,
        is_three_flush: bool,
        is_four_board_danger: bool,
    ) {
        let is_preflop = round == 1;

        if is_preflop && to_call <= bb {
            let pf_tier = preflop_strength_tier(bucket);
            if pf_tier <= 3 {
                let target_idx = legal_actions
                    .iter()
                    .position(|&a| a == RAISE_HALF_POT)
                    .or_else(|| legal_actions.iter().position(|&a| a == RAISE_THIRD_POT))
                    .or_else(|| legal_actions.iter().position(|&a| a == RAISE_MIN));
                if let Some(t_i) = target_idx {
                    let mut shifted = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a == CALL_CHECK {
                            let s = probs[i] * 0.98;
                            probs[i] -= s;
                            shifted += s;
                        } else if a == RAISE_MIN && i != t_i {
                            let s = probs[i] * 0.90;
                            probs[i] -= s;
                            shifted += s;
                        } else if a == RAISE_THIRD_POT && i != t_i {
                            let s = probs[i] * 0.50;
                            probs[i] -= s;
                            shifted += s;
                        }
                    }
                    probs[t_i] = (probs[t_i] + shifted).max(0.80);
                }
            } else if pf_tier <= 5 && bucket <= 105 {
                let target_idx = legal_actions
                    .iter()
                    .position(|&a| a == RAISE_THIRD_POT)
                    .or_else(|| legal_actions.iter().position(|&a| a == RAISE_HALF_POT))
                    .or_else(|| legal_actions.iter().position(|&a| a == RAISE_MIN));
                if let Some(t_i) = target_idx {
                    let mut shifted = 0.0f64;
                    if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                        let s = probs[c_i] * 0.85;
                        probs[c_i] -= s;
                        shifted += s;
                    }
                    if let Some(m_i) = legal_actions.iter().position(|&a| a == RAISE_MIN) {
                        if m_i != t_i {
                            let s = probs[m_i] * 0.70;
                            probs[m_i] -= s;
                            shifted += s;
                        }
                    }
                    probs[t_i] += shifted;
                }
            }
        } else if !is_preflop && to_call == 0 && !is_four_board_danger {
            let is_three_straight = board
                .map(|b| {
                    let mut ranks = [false; 13];
                    for c in b {
                        ranks[c.rank as usize] = true;
                    }
                    (0..=9).any(|s| (s..s + 4).filter(|&r| ranks[r]).count() >= 3)
                })
                .unwrap_or(false);
            let is_dangerous_river = round == 4
                && (board.map(is_dangerous_board).unwrap_or(false)
                    || is_three_flush
                    || is_three_straight);

            if bucket <= 14 {
                /* Pure air against a station who never folds: stop burning chips on air bluffs! */
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        let s = probs[i] * 0.90;
                        probs[i] -= s;
                        shifted += s;
                    }
                }
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] += shifted;
                }
            } else if bucket >= 65
                && bucket != 73
                && !(is_three_flush && bucket < 79)
                && !(round == 4
                    && (74..=78).contains(&bucket)
                    && board
                        .map(|b| detect_board_texture(b).is_paired)
                        .unwrap_or(false))
            {
                /* Trips / Set / Straight / Flush / Full House+: relentless heavy value */
                let hp_idx = legal_actions.iter().position(|&a| a == RAISE_HALF_POT);
                let ai_idx = legal_actions.iter().position(|&a| a == ALL_IN);
                let tp_idx = legal_actions.iter().position(|&a| a == RAISE_THIRD_POT);
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a == CALL_CHECK {
                        let s = probs[i] * 0.98;
                        probs[i] -= s;
                        shifted += s;
                    } else if a == RAISE_MIN {
                        let s = probs[i] * 0.95;
                        probs[i] -= s;
                        shifted += s;
                    } else if a == RAISE_THIRD_POT && (hp_idx.is_some() || ai_idx.is_some()) {
                        let s = probs[i] * 0.65;
                        probs[i] -= s;
                        shifted += s;
                    }
                }
                if let Some(hp_i) = hp_idx {
                    if let Some(ai_i) = ai_idx {
                        probs[hp_i] = (probs[hp_i] + shifted * 0.70).max(0.65);
                        probs[ai_i] += shifted * 0.30;
                    } else {
                        probs[hp_i] = (probs[hp_i] + shifted).max(0.85);
                    }
                } else if let Some(tp_i) = tp_idx {
                    probs[tp_i] = (probs[tp_i] + shifted).max(0.85);
                }
            } else if (57..=64).contains(&bucket) && !is_three_flush {
                /* Two Pair: barrel large value (RAISE_HALF_POT / RAISE_THIRD_POT) */
                let target_idx = if is_dangerous_river && bucket < 61 {
                    legal_actions.iter().position(|&a| a == RAISE_THIRD_POT)
                } else {
                    legal_actions
                        .iter()
                        .position(|&a| a == RAISE_HALF_POT)
                        .or_else(|| legal_actions.iter().position(|&a| a == RAISE_THIRD_POT))
                };
                if let Some(t_i) = target_idx {
                    let mut shifted = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a == CALL_CHECK {
                            let s = probs[i] * 0.98;
                            probs[i] -= s;
                            shifted += s;
                        } else if a == RAISE_MIN && i != t_i {
                            let s = probs[i] * 0.90;
                            probs[i] -= s;
                            shifted += s;
                        } else if a == RAISE_THIRD_POT && i != t_i {
                            let s = probs[i] * 0.55;
                            probs[i] -= s;
                            shifted += s;
                        }
                    }
                    probs[t_i] = (probs[t_i] + shifted).max(0.80);
                }
            } else if (45..=56).contains(&bucket) && !is_three_flush {
                /* Top Pair Good/Top Kicker or Overpair: extract multi-street value instead of checking behind! */
                let mut shifted = 0.0f64;
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    let s = probs[c_i] * 0.98;
                    probs[c_i] -= s;
                    shifted += s;
                }
                if let Some(m_i) = legal_actions.iter().position(|&a| a == RAISE_MIN) {
                    let s = probs[m_i] * 0.85;
                    probs[m_i] -= s;
                    shifted += s;
                }
                let hp_idx = legal_actions.iter().position(|&a| a == RAISE_HALF_POT);
                let tp_idx = legal_actions.iter().position(|&a| a == RAISE_THIRD_POT);
                if !is_dangerous_river {
                    if let Some(hp_i) = hp_idx {
                        if let Some(tp_i) = tp_idx {
                            probs[hp_i] = (probs[hp_i] + shifted * 0.65).max(0.60);
                            probs[tp_i] += shifted * 0.35;
                        } else {
                            probs[hp_i] = (probs[hp_i] + shifted).max(0.80);
                        }
                    } else if let Some(tp_i) = tp_idx {
                        probs[tp_i] = (probs[tp_i] + shifted).max(0.80);
                    }
                } else if let Some(tp_i) = tp_idx {
                    probs[tp_i] = (probs[tp_i] + shifted).max(0.75);
                }
            } else if (35..=44).contains(&bucket)
                && bucket != 39
                && bucket != 40
                && !is_three_flush
                && !is_dangerous_river
            {
                /* Top Pair Weak Kicker or Strong Middle Pair: consistent thin value (33% pot) */
                let target_idx = legal_actions
                    .iter()
                    .position(|&a| a == RAISE_THIRD_POT)
                    .or_else(|| legal_actions.iter().position(|&a| a == RAISE_MIN));
                if let Some(t_i) = target_idx {
                    if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                        let s = probs[c_i] * 0.68;
                        probs[c_i] -= s;
                        probs[t_i] = (probs[t_i] + s).max(0.60);
                    }
                }
            }
        }
    }

    /* Postflop Heavy-Bet / Multi-Street Barrel Defense
     * & Unconditional Pot-Commitment Rescue */
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_postflop_barrel_and_pot_commitment_guards(
        &self,
        probs: &mut [f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        bb: i32,
        pot_odds: f64,
        is_three_flush: bool,
        never_fold_postflop: bool,
    ) {
        let is_preflop = round == 1;

        /* 1. Flop heavy-bet defense:
         * - Fold bare Bottom Pair / Underpair without draw (28..=32), Pocket Pair under 2+ overcards (34),
         *   or Weak 2nd/Middle Pair (35) when facing >= ~58% pot bet (pot_odds >= 0.27, to_call >= 3 BB).
         * - Fold 2nd Pair Good Kicker (36) when facing a Flop overbet (to_call >= 5 BB, pot_odds >= 0.35). */
        if round == 2 && to_call >= bb * 3 {
            let should_fold_flop = (((28..=32).contains(&bucket) || bucket == 34 || bucket == 35)
                && pot_odds >= 0.27)
                || (bucket == 36 && to_call >= bb * 5 && pot_odds >= 0.35);
            if should_fold_flop {
                if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                    for p in probs.iter_mut() {
                        *p = 0.0;
                    }
                    probs[f_i] = 1.0;
                }
            }
        }

        /* 2. Turn & River barrel defense:
         * - Fold Weak / 2nd / 3rd / Bottom Pair without draw ((28..=36).contains(&bucket) && bucket != 33)
         *   when facing a medium/large Turn or River bet (pot_odds >= 0.24, to_call >= 2 BB).
         * - On a 3-flush Turn/River (`is_three_flush`), fold bare Top Pair Weak/Mid Kicker without a flush redraw
         *   (bucket <= 42) when facing a heavy barrel (to_call >= 4 BB, pot_odds >= 0.26). */
        if round >= 3 && to_call >= bb * 2 {
            let should_fold_weak_pair = (28..=36).contains(&bucket)
                && bucket != 33
                && pot_odds >= 0.24
                && (round == 4 || pot_odds >= 0.25 || to_call >= bb * 4);
            let should_fold_three_flush_tp =
                is_three_flush && bucket <= 42 && to_call >= bb * 4 && pot_odds >= 0.26;
            if should_fold_weak_pair || should_fold_three_flush_tp {
                if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                    for p in probs.iter_mut() {
                        *p = 0.0;
                    }
                    probs[f_i] = 1.0;
                }
            }
        }

        /* 3. Final Unconditional Pot-Commitment Rescue:
         * Ensures no intermediate shove/danger guard can force a fold when pot_odds <= 12%
         * (e.g. 2 chips into 10,940, or 600 chips into 9,400). */
        if to_call > 0
            && (pot_odds <= 0.12
                || (pot_odds <= 0.15 && (is_preflop || bucket >= 15))
                || never_fold_postflop)
        {
            if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                let f_prob = probs[f_i];
                probs[f_i] = 0.0;
                if let Some(c_i) = legal_actions
                    .iter()
                    .position(|&a| a == CALL_CHECK || a == ALL_IN)
                {
                    probs[c_i] = (probs[c_i] + f_prob).max(0.95);
                }
            }
        }
    }
}
