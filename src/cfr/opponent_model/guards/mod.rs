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
            pot_odds,
            is_three_flush,
            is_passive_station,
            reasons,
        );

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
