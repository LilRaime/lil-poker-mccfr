/*! Priority Tier 1: Soft Value / Sizing Shifts, Button Steals & River Nut Blocker Bluffs */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::helpers::{action_idx, first_action_idx, shift_action_to};
use crate::cfr::abstraction::{
    BUCKET_STRONG_DRAW_MIN, BoardFlushTexture, detect_board_texture, nash_open_push_max_bb,
    preflop_strength_tier,
};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, ROUND_FLOP,
};

impl OpponentTracker {
    pub(super) fn apply_tier1_soft_boosts(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.is_passive_station && self.is_guard_enabled("PASSIVE_STATION_BOOST") {
            reasons.push("PASSIVE_STATION_BOOST");
            Self::apply_passive_station_boost(probs, ctx);
        }

        self.apply_pot_commit_early(probs, ctx, reasons);
        self.apply_preflop_steal_and_trap(probs, ctx, reasons);
        self.apply_river_nut_blocker_bluff(probs, ctx, reasons);
        self.apply_flop_wet_set_value(probs, ctx, reasons);
    }

    fn apply_pot_commit_early(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.to_call > 0
            && ctx.allow_pot_commit_rescue
            && (ctx.pot_odds <= 0.12
                || (ctx.pot_odds <= 0.15
                    && (ctx.is_preflop || ctx.bucket >= BUCKET_STRONG_DRAW_MIN))
                || ctx.never_fold_postflop)
            && self.is_guard_enabled("POT_COMMIT_EARLY")
            && shift_action_to(probs, ctx.legal_actions, FOLD, &[CALL_CHECK, ALL_IN]) > 0.0
        {
            reasons.push("POT_COMMIT_EARLY");
        }
    }

    fn apply_preflop_steal_and_trap(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if !ctx.is_preflop {
            return;
        }
        let max_steal_tier = if ctx.is_hyper_aggressor {
            if ctx.active_opponents == 1 { 5 } else { 3 }
        } else if ctx.active_opponents == 1 {
            6
        } else if ctx.active_opponents == 2 {
            4
        } else {
            3
        };
        let steal_max_call = ctx.bb / 2;
        let pf_tier_now = preflop_strength_tier(ctx.bucket);

        if (10..=19).contains(&ctx.eff_stack_bb)
            && ctx.to_call > 0
            && ctx.to_call <= steal_max_call
            && ctx.is_hyper_aggressor
            && pf_tier_now <= 5
            && ctx.bucket >= 4
            && nash_open_push_max_bb(ctx.bucket, ctx.active_opponents) + 2 >= ctx.eff_stack_bb
            && self.is_guard_enabled("PF_BUTTON_STEAL")
        {
            if let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN) {
                reasons.push("PF_BUTTON_STEAL");
                let mut shifted = 0.0f64;
                for (i, &a) in ctx.legal_actions.iter().enumerate() {
                    if a == CALL_CHECK || (RAISE_MIN..ALL_IN).contains(&a) {
                        let s = probs[i] * 0.85;
                        probs[i] -= s;
                        shifted += s;
                    }
                }
                probs[ai_i] += shifted;
            }
        } else if (25..=40).contains(&ctx.eff_stack_bb)
            && ctx.to_call > 0
            && ctx.to_call <= steal_max_call
            && ctx.is_hyper_aggressor
            && pf_tier_now == 1
            && ctx.bucket <= 1
            && self.is_guard_enabled("PF_SB_AGGRESSOR_TRAP")
        {
            Self::apply_sb_aggressor_limp_trap(probs, ctx, reasons);
        } else {
            if self.is_guard_enabled("PF_BUTTON_STEAL") {
                Self::compact_hu_sb_open_sizing(probs, ctx);
            }
            if ctx.to_call <= steal_max_call
                && pf_tier_now <= max_steal_tier
                && self.is_guard_enabled("PF_BUTTON_STEAL")
            {
                self.apply_sb_button_steal_shift(probs, ctx, pf_tier_now, reasons);
            } else if ctx.active_opponents == 1
                && ctx.to_call > steal_max_call
                && ctx.to_call <= ctx.bb * 3
                && (10..=20).contains(&ctx.eff_stack_bb)
                && (pf_tier_now <= 2
                    || (pf_tier_now == 3 && (ctx.is_hyper_aggressor || ctx.eff_stack_bb <= 16)))
                && self.is_guard_enabled("PF_BUTTON_STEAL")
            {
                Self::apply_short_3bet_resteal_shove(probs, ctx, reasons);
            } else if ctx.active_opponents == 1
                && ctx.to_call > steal_max_call
                && ctx.to_call <= ctx.bb * 3
                && self.faced_3bet_count >= 3
                && self.fold_to_3bet_ratio() > 0.60
                && (2..=4).contains(&pf_tier_now)
                && self.is_guard_enabled("PF_BUTTON_STEAL")
            {
                Self::apply_fold_to_3bet_exploit(probs, ctx, reasons);
            }
        }
    }

    fn compact_hu_sb_open_sizing(probs: &mut [f64], ctx: &GuardContext<'_>) {
        if ctx.active_opponents != 1
            || ctx.to_call == 0
            || ctx.to_call > ctx.bb / 2
            || ctx.pot > ctx.bb * 2
            || ctx.eff_stack_bb <= 15
            || ctx.is_passive_station
        {
            return;
        }
        let Some(min_i) = action_idx(ctx.legal_actions, RAISE_MIN) else {
            return;
        };
        let mut shifted = 0.0f64;
        for (i, &a) in ctx.legal_actions.iter().enumerate() {
            if a == RAISE_THIRD_POT || a == RAISE_HALF_POT {
                shifted += probs[i];
                probs[i] = 0.0;
            }
        }
        probs[min_i] += shifted;
    }

    fn apply_sb_aggressor_limp_trap(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) else {
            return;
        };
        reasons.push("PF_SB_AGGRESSOR_TRAP");
        let mut shifted = 0.0f64;
        for (i, &a) in ctx.legal_actions.iter().enumerate() {
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

    fn apply_sb_button_steal_shift(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        pf_tier_now: u8,
        reasons: &mut Vec<&'static str>,
    ) {
        let fold_idx = action_idx(ctx.legal_actions, FOLD);
        let call_idx = action_idx(ctx.legal_actions, CALL_CHECK);
        let Some(r_i) = first_action_idx(ctx.legal_actions, &[RAISE_MIN, RAISE_THIRD_POT]) else {
            return;
        };
        /* Only intervene when the raw strategy is overly passive or SB holds Tier <= 4 (or deep Tier 5); leave strong blueprint raises untouched. */
        let passive_mass = fold_idx.map_or(0.0, |i| probs[i]) + call_idx.map_or(0.0, |i| probs[i]);
        let force_sb_open_tier =
            ctx.to_call > 0 && (pf_tier_now <= 4 || (pf_tier_now == 5 && ctx.eff_stack_bb > 15));
        if !(force_sb_open_tier
            || passive_mass > 0.55
            || ctx.is_passive_station
            || ctx.is_tight_passive_bb)
        {
            return;
        }
        reasons.push("PF_BUTTON_STEAL");
        if let Some(f_i) = fold_idx
            && probs[f_i] > 0.05
        {
            probs[r_i] += probs[f_i] * 0.85;
            probs[f_i] *= 0.15;
        }
        if ctx.to_call > 0 {
            if let Some(c_i) = call_idx {
                let limp_shift = if pf_tier_now <= 3 {
                    0.98
                } else if pf_tier_now == 4 || (pf_tier_now == 5 && ctx.eff_stack_bb > 15) {
                    0.95
                } else {
                    0.55
                };
                let shifted = probs[c_i] * limp_shift;
                probs[r_i] += shifted;
                probs[c_i] -= shifted;
            }
            if (10..=19).contains(&ctx.eff_stack_bb)
                && ctx.bucket >= 4
                && nash_open_push_max_bb(ctx.bucket, ctx.active_opponents) >= ctx.eff_stack_bb
                && let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN)
            {
                let s = probs[r_i] * 0.55;
                probs[r_i] -= s;
                probs[ai_i] += s;
            }
        } else if let Some(c_i) = call_idx {
            let allow_tier6_iso = ctx.active_opponents == 1
                && ctx.is_frequent_sb_limper
                && ctx.eff_stack_bb > 12
                && pf_tier_now == 6
                && (ctx.bucket <= 117
                    || matches!(ctx.bucket, 125 | 134 | 141 | 148 | 154 | 159 | 163));
            let limp_shift = if pf_tier_now <= 3 {
                0.55
            } else if pf_tier_now <= 5 && ctx.is_frequent_sb_limper {
                0.50
            } else if pf_tier_now <= 5 && self.sb_total_hands >= 4 && self.sb_limp_ratio() > 0.25 {
                0.40
            } else if allow_tier6_iso {
                0.35
            } else {
                0.0
            };
            if limp_shift > 0.0 {
                let shifted = probs[c_i] * limp_shift;
                probs[r_i] += shifted;
                probs[c_i] -= shifted;
            }
        }
    }

    fn apply_short_3bet_resteal_shove(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN) else {
            return;
        };
        reasons.push("PF_BUTTON_STEAL");
        let mut shifted = 0.0f64;
        for (i, &a) in ctx.legal_actions.iter().enumerate() {
            if a == RAISE_MIN || a == RAISE_THIRD_POT {
                let s = probs[i] * 0.70;
                probs[i] -= s;
                shifted += s;
            } else if a == CALL_CHECK && ctx.eff_stack_bb <= 16 {
                let s = probs[i] * 0.45;
                probs[i] -= s;
                shifted += s;
            }
        }
        probs[ai_i] += shifted;
    }

    fn apply_fold_to_3bet_exploit(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        let Some(r_i) = first_action_idx(ctx.legal_actions, &[RAISE_MIN, RAISE_THIRD_POT]) else {
            return;
        };
        reasons.push("PF_BUTTON_STEAL");
        let mut shifted = 0.0f64;
        if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
            let s = probs[c_i] * 0.30;
            probs[c_i] -= s;
            shifted += s;
        }
        if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
            let s = probs[f_i] * 0.25;
            probs[f_i] -= s;
            shifted += s;
        }
        probs[r_i] += shifted;
    }

    fn apply_river_nut_blocker_bluff(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if !ctx.has_river_nut_blocker || !self.is_guard_enabled("RIVER_NUT_BLOCKER_BLUFF") {
            return;
        }
        let Some(r_i) = first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, RAISE_MIN]) else {
            return;
        };
        reasons.push("RIVER_NUT_BLOCKER_BLUFF");
        probs[r_i] = (probs[r_i] * 1.65).max(0.25);
        if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
            probs[c_i] *= 0.65;
        }
        if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
            probs[f_i] *= 0.50;
        }
    }

    fn apply_flop_wet_set_value(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.round != ROUND_FLOP
            || ctx.to_call != 0
            || !(68..=72).contains(&ctx.bucket)
            || !self.is_guard_enabled("FLOP_WET_SET_VALUE")
        {
            return;
        }
        let is_wet_flop = ctx.board.is_some_and(|b| {
            let t = detect_board_texture(b);
            t.flush_texture != BoardFlushTexture::Rainbow || t.is_connected
        });
        if !is_wet_flop {
            return;
        }
        let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) else {
            return;
        };
        if probs[c_i] <= 0.40 {
            return;
        }
        let tp_idx = action_idx(ctx.legal_actions, RAISE_THIRD_POT);
        let hp_idx = action_idx(ctx.legal_actions, RAISE_HALF_POT);
        let shift = probs[c_i] * 0.65;
        match (tp_idx, hp_idx) {
            (Some(t_i), Some(h_i)) => {
                reasons.push("FLOP_WET_SET_VALUE");
                probs[c_i] -= shift;
                probs[t_i] += shift * 0.50;
                probs[h_i] += shift * 0.50;
            }
            (Some(t_i), None) => {
                reasons.push("FLOP_WET_SET_VALUE");
                probs[c_i] -= shift;
                probs[t_i] += shift;
            }
            (None, Some(h_i)) => {
                reasons.push("FLOP_WET_SET_VALUE");
                probs[c_i] -= shift;
                probs[h_i] += shift;
            }
            (None, None) => {}
        }
    }
}
