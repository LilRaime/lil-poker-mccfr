/*! Priority Tier 2A: Preflop Open/Raise Discipline & Stack-Commit Guards */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::helpers::{action_idx, first_action_idx, force_action, zero_and_sum};
use crate::cfr::abstraction::{
    nash_call_shove_max_bb, nash_open_push_max_bb, preflop_strength_tier,
};
use crate::game::config::{ALL_IN, CALL_CHECK, FOLD, RAISE_MIN, RAISE_THIRD_POT};

impl OpponentTracker {
    pub(super) fn apply_preflop_discipline_guard(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        let pf_tier = preflop_strength_tier(ctx.bucket);
        if !Self::apply_sb_open_discipline(probs, ctx, pf_tier, reasons) {
            Self::apply_preflop_facing_raise_discipline(probs, ctx, pf_tier, reasons);
        }
        Self::apply_preflop_pair_and_jam_caps(probs, ctx, pf_tier, reasons);
    }

    fn apply_sb_open_discipline(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        pf_tier: u8,
        reasons: &mut Vec<&'static str>,
    ) -> bool {
        let is_sb_open = ctx.active_opponents == 1
            && ctx.to_call > 0
            && ctx.to_call <= ctx.bb / 2
            && ctx.pot <= ctx.bb * 2;
        let is_multiway_cold_3bet_shove =
            ctx.is_preflop && ctx.active_opponents >= 2 && ctx.pot > ctx.to_call + ctx.bb * 3;
        let raw_max_nash_bb = nash_call_shove_max_bb(
            ctx.bucket,
            ctx.active_opponents,
            ctx.is_known_maniac || ctx.is_extreme_shove_bot,
        );
        let max_nash_bb = if is_multiway_cold_3bet_shove {
            (raw_max_nash_bb * 2) / 3
        } else {
            raw_max_nash_bb
        };
        let short_nash_call_ok = (ctx.is_allin_call
            || !ctx.can_raise
            || (ctx.is_short_stack && ctx.to_call >= ctx.bb * 6))
            && ctx.to_call > 0
            && ctx.to_call <= max_nash_bb * ctx.bb
            && pf_tier <= 5;

        let allow_hu_bb_tier6_iso = ctx.active_opponents == 1
            && ctx.is_frequent_sb_limper
            && ctx.eff_stack_bb > 12
            && pf_tier == 6
            && (ctx.bucket <= 117 || matches!(ctx.bucket, 125 | 134 | 141 | 148 | 154 | 159 | 163));

        if ctx.to_call == 0 && pf_tier >= 6 && !allow_hu_bb_tier6_iso {
            reasons.push("PF_DISCIPLINE_GUARD");
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
            true
        } else if short_nash_call_ok {
            if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                let f_w = probs[f_i];
                probs[f_i] = 0.0;
                if let Some(c_i) = first_action_idx(ctx.legal_actions, &[CALL_CHECK, ALL_IN]) {
                    probs[c_i] += f_w;
                }
            }
            true
        } else if is_sb_open
            && !ctx.is_tight_passive_bb
            && ctx.eff_stack_bb <= 35
            && ((ctx.is_extreme_shove_bot && pf_tier >= 6)
                || (ctx.is_known_maniac
                    && (pf_tier == 8
                        || (pf_tier == 7 && ctx.bucket >= 120 && ctx.eff_stack_bb <= 25))))
            && nash_open_push_max_bb(ctx.bucket, ctx.active_opponents) < ctx.eff_stack_bb
        {
            reasons.push("PF_DISCIPLINE_GUARD");
            force_action(probs, ctx.legal_actions, FOLD);
            true
        } else if is_sb_open && ctx.is_hyper_aggressor && pf_tier == 6 && ctx.eff_stack_bb > 15 {
            reasons.push("PF_DISCIPLINE_GUARD");
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                let limp_w = probs[c_i];
                probs[c_i] = 0.0;
                if ctx.bucket <= 117
                    && let Some(r_i) =
                        first_action_idx(ctx.legal_actions, &[RAISE_MIN, RAISE_THIRD_POT])
                {
                    probs[r_i] += limp_w;
                } else if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                    probs[f_i] += limp_w;
                }
            }
            if ctx.bucket > 117
                && let Some(r_i) = action_idx(ctx.legal_actions, RAISE_MIN)
                && let Some(f_i) = action_idx(ctx.legal_actions, FOLD)
            {
                let cut = probs[r_i] * 0.65;
                probs[r_i] -= cut;
                probs[f_i] += cut;
            }
            true
        } else if is_sb_open && pf_tier >= 7 {
            reasons.push("PF_DISCIPLINE_GUARD");
            Self::apply_sb_trash_steal_or_fold(probs, ctx, pf_tier);
            true
        } else {
            false
        }
    }

    fn apply_sb_trash_steal_or_fold(probs: &mut [f64], ctx: &GuardContext<'_>, pf_tier: u8) {
        if ctx.is_tight_passive_bb {
            if let Some(r_i) = first_action_idx(ctx.legal_actions, &[RAISE_MIN, RAISE_THIRD_POT]) {
                let steal_frac = if pf_tier == 7 { 0.85 } else { 0.70 };
                let mut shifted = 0.0f64;
                if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                    let s = probs[f_i] * steal_frac;
                    probs[f_i] -= s;
                    shifted += s;
                }
                if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                    let c_w = probs[c_i];
                    let s = c_w * steal_frac;
                    probs[c_i] = 0.0;
                    shifted += s;
                    if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                        probs[f_i] += c_w - s;
                    }
                }
                probs[r_i] += shifted;
            }
        } else if ctx.is_extreme_shove_bot
            || (ctx.is_known_maniac
                && (ctx.bucket >= 128 || (ctx.eff_stack_bb <= 25 && ctx.bucket >= 114)))
            || (pf_tier == 8
                && !(ctx.active_opponents == 1
                    && !ctx.is_known_maniac
                    && matches!(ctx.bucket, 119..=123 | 128..=130 | 136)))
        {
            force_action(probs, ctx.legal_actions, FOLD);
        } else if !ctx.is_passive_station {
            let is_playable_hu_steal = ctx.active_opponents == 1
                && (ctx.bucket < 91
                    || (111..=123).contains(&ctx.bucket)
                    || (128..=130).contains(&ctx.bucket)
                    || ctx.bucket == 136);
            let is_short_suited_hu_open = ctx.active_opponents == 1
                && ctx.eff_stack_bb <= 15
                && ctx.bucket < 91
                && pf_tier == 7;
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                let limp_w = probs[c_i];
                probs[c_i] = 0.0;
                let steal_share = if is_short_suited_hu_open {
                    0.90
                } else if is_playable_hu_steal && pf_tier == 7 {
                    0.65
                } else if is_playable_hu_steal {
                    0.45
                } else {
                    0.35
                };
                if let Some(r_i) =
                    first_action_idx(ctx.legal_actions, &[RAISE_MIN, RAISE_THIRD_POT])
                {
                    probs[r_i] += limp_w * steal_share;
                }
                if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                    probs[f_i] += limp_w * (1.0 - steal_share);
                }
            }
            if is_playable_hu_steal
                && let (Some(f_i), Some(r_i)) = (
                    action_idx(ctx.legal_actions, FOLD),
                    first_action_idx(ctx.legal_actions, &[RAISE_MIN, RAISE_THIRD_POT]),
                )
            {
                let fold_shift = if is_short_suited_hu_open {
                    0.85
                } else if pf_tier == 7 {
                    0.30
                } else {
                    0.25
                };
                let s = probs[f_i] * fold_shift;
                probs[f_i] -= s;
                probs[r_i] += s;
            }
        }
    }

    fn apply_preflop_facing_raise_discipline(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        pf_tier: u8,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.to_call > 0 && pf_tier >= 7 {
            reasons.push("PF_DISCIPLINE_GUARD");
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            let is_hu_small_open = ctx.active_opponents == 1
                && ctx.to_call <= ctx.bb * 2
                && ctx.pot <= ctx.bb * 4
                && ctx.pot_odds <= 0.33
                && pf_tier == 7
                && ctx.bucket < 135;
            let is_hu_min_open_playable_t8 = ctx.active_opponents == 1
                && ctx.to_call <= ctx.bb
                && ctx.pot <= ctx.bb * 3
                && ctx.pot_odds <= 0.26
                && matches!(ctx.bucket, 121..=123 | 128..=130 | 136 | 144);
            let is_offsuit_trash_vs_3bet =
                ctx.pot >= ctx.bb * 6 && ctx.to_call >= ctx.bb * 2 && ctx.bucket >= 91;
            if is_hu_small_open {
                if let Some(c_i) = first_action_idx(ctx.legal_actions, &[CALL_CHECK, ALL_IN]) {
                    probs[c_i] += shifted;
                    if ctx.pot_odds <= 0.32
                        && let Some(f_i) = action_idx(ctx.legal_actions, FOLD)
                    {
                        let defend_frac = if ctx.pot_odds <= 0.26 { 0.80 } else { 0.65 };
                        let defend = probs[f_i] * defend_frac;
                        probs[f_i] -= defend;
                        probs[c_i] += defend;
                    }
                }
            } else if !is_offsuit_trash_vs_3bet
                && (ctx.pot_odds <= 0.22
                    || (ctx.pot_odds <= 0.26 && pf_tier <= 7)
                    || is_hu_min_open_playable_t8)
            {
                if let Some(c_i) = first_action_idx(ctx.legal_actions, &[CALL_CHECK, ALL_IN]) {
                    probs[c_i] += shifted;
                }
            } else if ctx.to_call > ctx.bb / 2
                && (pf_tier == 8 || ctx.pot_odds > 0.28 || is_offsuit_trash_vs_3bet)
            {
                force_fold_min(probs, ctx.legal_actions, shifted, 0.98);
            } else if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                probs[f_i] += shifted;
            }
        } else if ctx.to_call >= ctx.bb
            && ctx.to_call < ctx.bb * 3
            && (pf_tier == 6 || (96..=99).contains(&ctx.bucket))
        {
            reasons.push("PF_DISCIPLINE_GUARD");
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            let is_hu_connected_tier6_defend = ctx.active_opponents == 1
                && ctx.pot < ctx.bb * 6
                && ctx.to_call <= ctx.bb * 2
                && ctx.pot_odds <= 0.31
                && matches!(ctx.bucket, 141 | 148 | 154 | 159 | 163);
            let is_low_offsuit_tier6_vs_3bet = pf_tier == 6
                && ctx.bucket >= 141
                && ctx.pot >= ctx.bb * 6
                && ctx.to_call >= ctx.bb * 2
                && !ctx.is_extreme_shove_bot;
            if is_low_offsuit_tier6_vs_3bet
                || (pf_tier == 6
                    && ctx.bucket >= 141
                    && ctx.pot_odds > 0.28
                    && !is_hu_connected_tier6_defend)
            {
                force_fold_min(probs, ctx.legal_actions, shifted, 0.95);
            } else if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += shifted;
                if is_hu_connected_tier6_defend
                    && let Some(f_i) = action_idx(ctx.legal_actions, FOLD)
                {
                    let defend = probs[f_i] * 0.60;
                    probs[f_i] -= defend;
                    probs[c_i] += defend;
                }
            }
        } else if ctx.to_call >= ctx.bb * 3 && pf_tier >= 6 {
            reasons.push("PF_DISCIPLINE_GUARD");
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if ctx.pot_odds > 0.28 && !ctx.is_extreme_shove_bot {
                force_fold_min(probs, ctx.legal_actions, shifted, 0.95);
            } else if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += shifted;
            }
        } else if ctx.to_call >= ctx.bb * 6
            && ctx.pot_odds > 0.36
            && (pf_tier >= 4 || ctx.active_opponents >= 2)
        {
            let is_multiway = ctx.active_opponents >= 2 && ctx.pot > ctx.to_call + ctx.bb * 3;
            let raw_nash = nash_call_shove_max_bb(
                ctx.bucket,
                ctx.active_opponents,
                ctx.is_known_maniac || ctx.is_extreme_shove_bot,
            );
            let max_nash_bb = if is_multiway {
                (raw_nash * 2) / 3
            } else {
                raw_nash
            };
            let nash_ok =
                (ctx.is_allin_call || !ctx.can_raise) && ctx.to_call <= max_nash_bb * ctx.bb;
            let defer_to_hu_shove_defense = ctx.active_opponents == 1
                && (ctx.is_allin_call || !ctx.can_raise || ctx.is_short_stack)
                && ((ctx.is_extreme_shove_bot && pf_tier <= 5)
                    || (ctx.is_known_maniac && pf_tier == 4));
            if !nash_ok && pf_tier >= 4 && !defer_to_hu_shove_defense {
                reasons.push("PF_DISCIPLINE_GUARD");
                let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
                force_fold_min(probs, ctx.legal_actions, shifted, 0.95);
            }
        } else if !ctx.is_allin_call
            && ctx.pot >= ctx.bb * 15
            && ctx.to_call >= ctx.bb * 5
            && ctx.pot_odds > 0.18
            && pf_tier >= 5
            && !ctx.is_extreme_shove_bot
        {
            reasons.push("PF_DISCIPLINE_GUARD");
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            force_fold_min(probs, ctx.legal_actions, shifted, 0.95);
        }
    }

    fn apply_preflop_pair_and_jam_caps(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        pf_tier: u8,
        reasons: &mut Vec<&'static str>,
    ) {
        Self::apply_preflop_pair_setmine_guard(probs, ctx, reasons);
        Self::apply_preflop_open_jam_cap(probs, ctx, pf_tier);
        Self::apply_preflop_deep_3bet_caps(probs, ctx, pf_tier, reasons);
    }

    fn apply_preflop_pair_setmine_guard(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        /* Small/mid pocket pairs (66..22) facing an open raise: forbid bloated 3-bets; set-mine */
        if !(ctx.to_call >= ctx.bb && (8..=12).contains(&ctx.bucket)) {
            return;
        }
        reasons.push("PF_DISCIPLINE_GUARD");
        let mut shifted = 0.0f64;
        for (i, &a) in ctx.legal_actions.iter().enumerate() {
            if a >= RAISE_THIRD_POT {
                shifted += probs[i];
                probs[i] = 0.0;
            } else if a == RAISE_MIN && probs[i] > 0.05 {
                shifted += probs[i] - 0.05;
                probs[i] = 0.05;
            }
        }
        if shifted > 0.0 {
            let target = if ctx.pot_odds <= 0.36 && ctx.to_call <= ctx.bb * 5 {
                CALL_CHECK
            } else {
                FOLD
            };
            if let Some(t_i) = action_idx(ctx.legal_actions, target) {
                probs[t_i] += shifted;
            }
        }
        if ctx.active_opponents == 1
            && ctx.to_call <= ctx.bb * 3
            && ctx.pot_odds <= 0.40
            && let (Some(f_i), Some(c_i)) = (
                action_idx(ctx.legal_actions, FOLD),
                action_idx(ctx.legal_actions, CALL_CHECK),
            )
            && probs[f_i] > 0.0
        {
            probs[c_i] += probs[f_i];
            probs[f_i] = 0.0;
        }
    }

    fn apply_preflop_open_jam_cap(probs: &mut [f64], ctx: &GuardContext<'_>, pf_tier: u8) {
        if ctx.eff_stack_bb > 20
            && (ctx.to_call <= ctx.bb
                || (ctx.to_call <= ctx.bb * 4 && (ctx.bucket >= 3 || ctx.active_opponents >= 2)))
            && let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN)
        {
            let s = probs[ai_i];
            if s > 0.0 {
                probs[ai_i] = 0.0;
                if let Some(t_i) =
                    first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK])
                {
                    probs[t_i] += s;
                }
            }
        }
        if ctx.eff_stack_bb > 20
            && !ctx.is_allin_call
            && (pf_tier >= 4 || (pf_tier >= 3 && (ctx.is_known_maniac || ctx.is_extreme_shove_bot)))
            && let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN)
        {
            let s = probs[ai_i];
            if s > 0.0 {
                probs[ai_i] = 0.0;
                let target = if ctx.pot_odds <= 0.34 && pf_tier <= 4 {
                    CALL_CHECK
                } else {
                    FOLD
                };
                if let Some(t_i) = first_action_idx(ctx.legal_actions, &[target, CALL_CHECK]) {
                    probs[t_i] += s;
                }
            }
        }
    }

    fn apply_preflop_deep_3bet_caps(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        pf_tier: u8,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.eff_stack_bb > 35
            && !ctx.is_allin_call
            && ctx.to_call > ctx.bb * 4
            && pf_tier == 2
            && ctx.bucket >= 4
            && let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN)
        {
            let s = probs[ai_i];
            if s > 0.0 {
                reasons.push("PF_DISCIPLINE_GUARD");
                probs[ai_i] = 0.0;
                match (
                    action_idx(ctx.legal_actions, RAISE_THIRD_POT),
                    action_idx(ctx.legal_actions, CALL_CHECK),
                ) {
                    (Some(ri), Some(ci)) => {
                        probs[ri] += s * 0.50;
                        probs[ci] += s * 0.50;
                    }
                    (Some(ri), None) => probs[ri] += s,
                    (None, Some(ci)) => probs[ci] += s,
                    (None, None) => {}
                }
            }
        }
        if !ctx.is_allin_call
            && ctx.eff_stack_bb >= 25
            && ctx.bucket == 6
            && ctx.to_call >= ctx.bb * 9
            && ctx.my_chips > 0
            && ctx.to_call * 10 >= ctx.my_chips * 3
            && ctx.pot_odds >= 0.35
            && !ctx.is_known_maniac
            && let (Some(c_i), Some(f_i)) = (
                action_idx(ctx.legal_actions, CALL_CHECK),
                action_idx(ctx.legal_actions, FOLD),
            )
            && probs[c_i] > 0.0
        {
            reasons.push("PF_DISCIPLINE_GUARD");
            probs[f_i] += probs[c_i];
            probs[c_i] = 0.0;
        }
        if !ctx.is_allin_call
            && ctx.pot >= ctx.bb * 15
            && ctx.to_call >= ctx.bb * 6
            && ctx.my_chips > ctx.to_call
        {
            let rem = ctx.my_chips - ctx.to_call;
            let new_pot = ctx.pot + ctx.to_call;
            if rem * 20 < new_pot * 11 {
                if pf_tier <= 2
                    || (pf_tier == 3
                        && (ctx.bucket <= 7 || ctx.is_known_maniac || ctx.is_extreme_shove_bot))
                {
                    if let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN) {
                        let mut shifted = 0.0f64;
                        for (i, &a) in ctx.legal_actions.iter().enumerate() {
                            if a == CALL_CHECK || (RAISE_MIN..ALL_IN).contains(&a) {
                                shifted += probs[i];
                                probs[i] = 0.0;
                            }
                        }
                        if shifted > 0.0 {
                            reasons.push("PF_DISCIPLINE_GUARD");
                            probs[ai_i] += shifted;
                        }
                    }
                } else if ctx.pot_odds > 0.22 {
                    reasons.push("PF_DISCIPLINE_GUARD");
                    let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
                    force_fold_min(probs, ctx.legal_actions, shifted, 0.95);
                }
            }
        }
    }
}

fn force_fold_min(probs: &mut [f64], legal_actions: &[u8], mut shifted: f64, min_fold: f64) {
    if let Some(f_i) = action_idx(legal_actions, FOLD) {
        if let Some(c_i) = action_idx(legal_actions, CALL_CHECK) {
            shifted += probs[c_i];
            probs[c_i] = 0.0;
        }
        probs[f_i] = (probs[f_i] + shifted).max(min_fold);
    }
}
