/* Priority Tier 1: Soft Value / Sizing Shifts, Button Steals & River Nut Blocker Bluffs */
use super::super::tracker::OpponentTracker;
use crate::cfr::abstraction::{detect_river_nut_blocker, preflop_strength_tier};
use crate::game::config::*;
use crate::game::holdem::Card;

impl OpponentTracker {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_tier1_soft_boosts(
        &self,
        probs: &mut [f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        pot: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        active_opponents: usize,
        bb: i32,
        pot_odds: f64,
        is_three_flush: bool,
        is_four_board_danger: bool,
        is_passive_station: bool,
        allow_pot_commit_rescue: bool,
        never_fold_postflop: bool,
        reasons: &mut Vec<&'static str>,
    ) {
        let is_preflop = round == 1;

        if is_passive_station && self.is_guard_enabled("PASSIVE_STATION_BOOST") {
            reasons.push("PASSIVE_STATION_BOOST");
            self.apply_passive_station_boost(
                probs,
                legal_actions,
                bucket,
                round,
                to_call,
                board,
                bb,
                is_three_flush,
                is_four_board_danger,
            );
        }

        if to_call > 0
            && allow_pot_commit_rescue
            && (pot_odds <= 0.12
                || (pot_odds <= 0.15 && (is_preflop || bucket >= 15))
                || never_fold_postflop)
            && self.is_guard_enabled("POT_COMMIT_EARLY")
        {
            reasons.push("POT_COMMIT_EARLY");
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

        /* Preflop Button Steal & Anti-Shove-Bot SB Limp Trap (Tier 1 Soft Shift):
         * - Against normal/passive opponents in Heads-Up, Button opens ~80% of hands profitably (tiers 1..=6).
         * - Against a proven hyper-aggressive 3-bet/shove opponent (total_hands >= 8, raise_ratio > 0.42),
         *   do NOT open-raise speculative steal hands (tiers 4..=6) into a 3-bet shove! Instead, limp-call
         *   tiers 4..=5 to see a cheap flop and only open-raise tiers 1..=3.
         */
        let is_hyper_aggressor =
            self.total_hands >= 8 && (self.raise_ratio() > 0.42 || self.pfr_ratio() > 0.45);
        let max_steal_tier = if is_hyper_aggressor {
            3
        } else if active_opponents == 1 {
            6
        } else if active_opponents == 2 {
            4
        } else {
            3
        };
        let steal_max_call = bb / 2;
        let pf_tier_now = preflop_strength_tier(bucket);

        if is_preflop
            && to_call <= steal_max_call
            && is_hyper_aggressor
            && (4..=5).contains(&pf_tier_now)
            && self.is_guard_enabled("PF_SB_AGGRESSOR_TRAP")
        {
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                reasons.push("PF_SB_AGGRESSOR_TRAP");
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i] * 0.85;
                        probs[i] *= 0.15;
                    } else if a == FOLD && probs[i] > 0.05 {
                        shifted += probs[i] * 0.70;
                        probs[i] *= 0.30;
                    }
                }
                probs[c_i] += shifted;
            }
        } else if is_preflop
            && to_call <= steal_max_call
            && pf_tier_now <= max_steal_tier
            && self.is_guard_enabled("PF_BUTTON_STEAL")
        {
            let fold_idx = legal_actions.iter().position(|&a| a == FOLD);
            let call_idx = legal_actions.iter().position(|&a| a == CALL_CHECK);
            let raise_idx = legal_actions
                .iter()
                .position(|&a| a == RAISE_MIN || a == RAISE_THIRD_POT);
            if let Some(r_i) = raise_idx {
                reasons.push("PF_BUTTON_STEAL");
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

        /* Blocker-aware River Bluffing (Tier 1 Soft Shift, Heads-Up only) */
        if round == 4
            && active_opponents == 1
            && to_call == 0
            && bucket < 28
            && !is_passive_station
            && (self.total_actions() < 6 || self.fold_ratio() >= 0.30)
            && pot <= bb * 12
            && self.is_guard_enabled("RIVER_NUT_BLOCKER_BLUFF")
        {
            if let (Some(h), Some(b)) = (hole, board) {
                if detect_river_nut_blocker(h, b) {
                    let raise_third = legal_actions.iter().position(|&a| a == RAISE_THIRD_POT);
                    let raise_min = legal_actions.iter().position(|&a| a == RAISE_MIN);
                    let check_idx = legal_actions.iter().position(|&a| a == CALL_CHECK);
                    let fold_idx = legal_actions.iter().position(|&a| a == FOLD);

                    if let Some(r_i) = raise_third.or(raise_min) {
                        reasons.push("RIVER_NUT_BLOCKER_BLUFF");
                        probs[r_i] = (probs[r_i] * 1.65).max(0.25);
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
    }
}
