/* Multi-Way Table Adaptation (3 to 6 Active Players: Range Tightening & Bluff Damping) */
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::{is_four_flush_board, preflop_strength_tier};
use crate::game::config::*;
use crate::game::holdem::Card;

impl OpponentTracker {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_multiway_adaptation(
        &self,
        probs: &mut [f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        board: Option<&[Card]>,
        active_opponents: usize,
        bb: i32,
        pot_odds: f64,
        is_three_flush: bool,
    ) {
        if active_opponents < 2 {
            return;
        }

        let is_preflop = round == 1;
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
                    bucket < 57 && !(bucket == 22 || bucket == 23 || bucket == 25 || bucket == 27)
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
}
