/*! Unified Post-Selection Action Guards Shared Between Live Client and Offline Simulation */
use super::super::policy::PolicyRequest;
use super::super::tracker::OpponentTracker;
use super::guard_id::{GuardId, GuardSet};
use crate::cfr::abstraction::{
    BUCKET_OVERPAIR_MIN, BUCKET_STRONG_DRAW_MIN, BUCKET_TOP_PAIR_MIN, BUCKET_TWO_PAIR_MIN,
    BUCKET_WEAK_PAIR_MIN, PostflopBucket, preflop_strength_tier,
};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_MIN, RAISE_THIRD_POT, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER,
    ROUND_TURN,
};
use crate::game::holdem::RoundHistory;

impl OpponentTracker {
    /** Applies street-history, SPR, and pot-commitment guardrails on the sampled discrete action
     * so live play and simulation execute an identical decision pipeline. */
    pub(in crate::cfr::opponent_model) fn apply_post_selection_guards(
        &self,
        req: &PolicyRequest<'_>,
        eff_chips: i32,
        bb: i32,
        chosen: &mut u8,
        reasons: &mut GuardSet,
    ) {
        if let (Some(my_player), Some(history)) = (req.my_player, req.history) {
            self.apply_post_selection_street_guards(req, my_player, history, chosen, reasons);
        }
        self.apply_post_selection_commit_and_spr_guards(req, eff_chips, bb, chosen, reasons);
    }

    fn apply_post_selection_street_guards(
        &self,
        req: &PolicyRequest<'_>,
        my_player: usize,
        history: &[RoundHistory; 4],
        chosen: &mut u8,
        reasons: &mut GuardSet,
    ) {
        if req.round == ROUND_RIVER
            && req.to_call == 0
            && req.bucket < BUCKET_TWO_PAIR_MIN
            && (my_player == 1 || req.bucket < BUCKET_TOP_PAIR_MIN)
            && *chosen >= RAISE_MIN
            && req.legal_actions.contains(&CALL_CHECK)
        {
            let turn_hist = history[2].as_slice();
            let turn_raises = turn_hist.iter().filter(|&&a| a >= RAISE_MIN).count();
            /* On Turn, OOP (my_player == 1) acts on even indices (0, 2, ...), IP on odd (1, 3, ...). */
            let opp_was_last_turn_raiser = turn_hist
                .iter()
                .enumerate()
                .rev()
                .find(|&(_, &a)| a >= RAISE_MIN)
                .is_some_and(|(idx, _)| {
                    let is_oop_action = idx % 2 == 0;
                    (my_player == 1 && !is_oop_action) || (my_player == 0 && is_oop_action)
                });
            if turn_raises >= 2 || (my_player == 1 && opp_was_last_turn_raiser) {
                *chosen = CALL_CHECK;
                reasons.insert(GuardId::RiverCheckToTurnAggressor);
            }
        }
        let pb = PostflopBucket::from_usize(req.bucket);
        if req.round == ROUND_FLOP
            && req.to_call == 0
            && my_player == 1
            && *chosen >= RAISE_MIN
            && *chosen < ALL_IN
            && req.bucket < BUCKET_TOP_PAIR_MIN
            && !pb.is_nut_or_combo_draw()
            && req.legal_actions.contains(&CALL_CHECK)
            && self.is_guard_enabled(GuardId::FlopOopMicroDonkSuppress)
        {
            let pf_hist = history[0].as_slice();
            let pf_raises = pf_hist.iter().filter(|&&a| a >= RAISE_MIN).count();
            let opp_was_pf_aggressor =
                pf_raises == 1 && pf_hist.first().copied().unwrap_or(0) >= RAISE_MIN;
            if opp_was_pf_aggressor {
                *chosen = CALL_CHECK;
                reasons.insert(GuardId::FlopOopMicroDonkSuppress);
            }
        }
    }

    fn apply_post_selection_commit_and_spr_guards(
        &self,
        req: &PolicyRequest<'_>,
        eff_chips: i32,
        bb: i32,
        chosen: &mut u8,
        reasons: &mut GuardSet,
    ) {
        if req.to_call > 0 && *chosen == FOLD && req.legal_actions.contains(&CALL_CHECK) {
            let new_pot = (req.pot + req.to_call).max(1);
            let is_postflop_micro_commit = req.round >= ROUND_FLOP
                && (req.to_call * 8 <= new_pot
                    || (req.to_call * 6 <= new_pot && req.bucket >= BUCKET_STRONG_DRAW_MIN)
                    || (eff_chips > 0 && eff_chips <= bb && req.pot >= bb * 4));
            let is_preflop_priced_in = req.round == ROUND_PREFLOP
                && (req.to_call <= bb * 2 || preflop_strength_tier(req.bucket) <= 6)
                && req.to_call * 6 <= new_pot;
            if is_postflop_micro_commit || is_preflop_priced_in {
                *chosen = CALL_CHECK;
                reasons.insert(GuardId::PotCommitDecisionRescue);
            }
        }
        let pb = if req.round == ROUND_PREFLOP {
            PostflopBucket(0)
        } else {
            PostflopBucket::from_usize(req.bucket)
        };
        if req.round >= ROUND_FLOP && eff_chips > 0 {
            let is_four_board_pair = req.round >= ROUND_TURN
                && matches!(
                    req.bucket,
                    BUCKET_WEAK_PAIR_MIN
                        | BUCKET_TOP_PAIR_MIN
                        | BUCKET_OVERPAIR_MIN
                        | BUCKET_TWO_PAIR_MIN
                );
            let is_bloated_flop_weak = req.round == ROUND_FLOP
                && ((pb.is_air_or_weak_draw() && (eff_chips <= req.pot * 2 || req.pot >= bb * 8))
                    || (pb.in_range(
                        BUCKET_WEAK_PAIR_MIN as u8,
                        PostflopBucket::DRY_MID_OR_UNDERPAIR_MAX,
                    ) && req.pot >= bb * 10));
            let is_sticky_or_station = self.tactical_profile().is_sticky_caller;
            let allow_hu_3bet_overcard_cbet = req.round == ROUND_FLOP
                && req.active_opponents == 1
                && !is_sticky_or_station
                && eff_chips > req.pot * 2
                && req.pot >= bb * 8
                && pb.as_u8() >= PostflopBucket::STRONG_OVERCARDS_MIN
                && pb.is_air_or_weak_draw()
                && *chosen <= RAISE_THIRD_POT;
            if req.to_call == 0
                && *chosen >= RAISE_MIN
                && !allow_hu_3bet_overcard_cbet
                && req.legal_actions.contains(&CALL_CHECK)
                && (is_four_board_pair
                    || is_bloated_flop_weak
                    || (pb.is_high_card()
                        && *chosen >= RAISE_THIRD_POT
                        && (eff_chips <= req.pot * 2 || req.pot >= bb * 8)))
            {
                *chosen = CALL_CHECK;
                reasons.insert(GuardId::LowSprBluffSuppress);
            } else if req.to_call > bb
                && (pb.is_air_or_weak_draw() || pb.is_crushed_pair())
                && *chosen == CALL_CHECK
                && req.legal_actions.contains(&FOLD)
            {
                let remaining = eff_chips - req.to_call;
                let new_pot = (req.pot + req.to_call).max(1);
                if req.to_call * 6 > new_pot
                    && (remaining * 2 < new_pot || req.to_call * 10 >= eff_chips * 3)
                {
                    *chosen = FOLD;
                    reasons.insert(GuardId::LowSprAirCallFold);
                }
            }
        }
    }
}
