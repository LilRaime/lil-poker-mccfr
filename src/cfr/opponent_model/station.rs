/* Anti-Passive / Calling-Station Value-Extraction Boost */
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::{is_dangerous_board, preflop_strength_tier};
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
            } else if bucket >= 65 && bucket != 73 && !(is_three_flush && bucket < 79) {
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
}

