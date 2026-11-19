/* Priority-Ordered Tactical Guards Architecture:
 * - Tier 1 (tier1_boosts): Soft value/sizing boosts, button steals, and blocker bluffs
 * - Tier 2 (tier2_shove & tier2_caps): Hard preflop/postflop shove defense, raise caps, and pot control
 * - Tier 3 (station::apply_postflop_barrel_and_pot_commitment_guards): Terminal barrel folds & pot-commitment rescue
 */
mod tier1_boosts;
mod tier2_caps;
mod tier2_shove;

use super::tracker::OpponentTracker;
use crate::cfr::abstraction::{is_four_flush_board, is_four_straight_board, preflop_strength_tier};
use crate::game::holdem::Card;

#[inline]
pub(super) fn river_nut_blocker_allowed(pot: i32, bb: i32, my_chips: i32) -> bool {
    pot <= bb * 12 || (my_chips > 0 && my_chips * 4 <= pot * 3)
}

impl OpponentTracker {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_preflop_and_postflop_guards(
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
        my_chips: i32,
        eff_allin_threshold: i32,
        pot_odds: f64,
        is_three_flush: bool,
        from_subgame: bool,
        reasons: &mut Vec<&'static str>,
    ) {
        let is_preflop = round == 1;
        let is_passive_station = self.is_passive_station();
        let is_four_flush_board_now = board.map(is_four_flush_board).unwrap_or(false);
        let is_paired_board_now = board
            .map(|b| crate::cfr::abstraction::detect_board_texture(b).is_paired)
            .unwrap_or(false);
        let is_four_board_danger = board
            .map(|b| is_four_straight_board(b) || is_four_flush_board(b))
            .unwrap_or(false);
        let never_fold_postflop = !is_preflop
            && (bucket >= 83
                || (bucket == 82 && !is_four_flush_board_now && to_call < pot * 2)
                || (bucket == 81 && !is_four_flush_board_now && !is_paired_board_now));
        let allow_pot_commit_rescue =
            !(is_preflop && preflop_strength_tier(bucket) >= 7 && to_call > bb * 2);

        /* Priority Tier 1: Soft Value / Sizing Shifts & Situational Exploits */
        self.apply_tier1_soft_boosts(
            probs,
            legal_actions,
            bucket,
            round,
            to_call,
            pot,
            hole,
            board,
            active_opponents,
            bb,
            my_chips,
            pot_odds,
            is_three_flush,
            is_four_board_danger,
            is_passive_station,
            allow_pot_commit_rescue,
            never_fold_postflop,
            reasons,
        );

        /* When raw_probs come from online SubgameSolver on Turn/River (round >= 3), trust the
         * solver's game-theoretic equilibrium: only apply catastrophic air-allin cap and
         * terminal pot-commitment rescue instead of overriding it with heuristic Tier 2/3 caps. */
        if from_subgame && round >= 3 {
            if bucket < 15 && self.is_guard_enabled("POST_ALLIN_BLUFF_CAP") {
                if let Some(ai_i) = legal_actions
                    .iter()
                    .position(|&a| a == crate::game::config::ALL_IN)
                {
                    let removed = probs[ai_i];
                    if removed > 0.0 {
                        probs[ai_i] = 0.0;
                        if let Some(c_i) = legal_actions
                            .iter()
                            .position(|&a| a == crate::game::config::CALL_CHECK)
                        {
                            probs[c_i] += removed;
                        }
                    }
                }
            }
            if to_call > 0
                && (pot_odds <= 0.12 || never_fold_postflop)
                && self.is_guard_enabled("POT_COMMIT_TERMINAL")
            {
                if let Some(f_i) = legal_actions
                    .iter()
                    .position(|&a| a == crate::game::config::FOLD)
                {
                    let f_prob = probs[f_i];
                    if f_prob > 0.0 {
                        reasons.push("POT_COMMIT_TERMINAL");
                        probs[f_i] = 0.0;
                        if let Some(c_i) = legal_actions.iter().position(|&a| {
                            a == crate::game::config::CALL_CHECK || a == crate::game::config::ALL_IN
                        }) {
                            probs[c_i] += f_prob;
                        }
                    }
                }
            }
            return;
        }

        /* Priority Tier 2A: Preflop Discipline & All-In / Heavy-Bet Shove Defense */
        self.apply_tier2_preflop_and_shove_defense(
            probs,
            legal_actions,
            bucket,
            round,
            to_call,
            pot,
            hole,
            board,
            active_opponents,
            bb,
            my_chips,
            eff_allin_threshold,
            pot_odds,
            is_three_flush,
            reasons,
        );

        /* Priority Tier 2B: Postflop Hard Raise Caps, Draw Odds & Pot Control */
        self.apply_tier2_postflop_caps_and_pot_control(
            probs,
            legal_actions,
            bucket,
            round,
            to_call,
            pot,
            hole,
            board,
            active_opponents,
            bb,
            my_chips,
            pot_odds,
            is_three_flush,
            is_passive_station,
            reasons,
        );

        /* Multi-Way Table Adaptation (3 to 6 players active) — runs BEFORE Tier 3 so terminal
         * pot-commitment rescues cannot be overwritten by multi-way folds */
        if active_opponents >= 2 {
            reasons.push("MULTIWAY_ADAPT");
            self.apply_multiway_adaptation(
                probs,
                legal_actions,
                bucket,
                round,
                to_call,
                board,
                active_opponents,
                bb,
                pot_odds,
                is_three_flush,
            );
        }

        /* Priority Tier 3: Terminal Barrel Defense & Unconditional Pot-Commitment */
        self.apply_postflop_barrel_and_pot_commitment_guards(
            probs,
            legal_actions,
            bucket,
            round,
            to_call,
            bb,
            pot_odds,
            is_three_flush,
            never_fold_postflop,
            reasons,
        );
    }
}
