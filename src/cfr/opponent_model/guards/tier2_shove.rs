/*! Priority Tier 2A: Preflop Discipline & All-In / Heavy-Bet Shove Defense */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::helpers::{action_idx, first_action_idx, force_action, zero_and_sum};
use crate::cfr::abstraction::{
    calculate_range_equity, nash_call_shove_max_bb, nash_open_push_max_bb, preflop_strength_tier,
};
use crate::game::config::{ALL_IN, CALL_CHECK, FOLD, RAISE_MIN, RAISE_THIRD_POT};

impl OpponentTracker {
    pub(super) fn apply_tier2_preflop_and_shove_defense(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.is_preflop && self.is_guard_enabled("PF_DISCIPLINE_GUARD") {
            self.apply_preflop_discipline_guard(probs, ctx, reasons);
        }

        let (is_shove_faced, is_stack_commit_draw_bet) = self.detect_shove_faced(ctx);
        if is_shove_faced && self.is_guard_enabled("SHOVE_DEFENSE_FOLD") {
            self.apply_shove_defense_fold(probs, ctx, is_stack_commit_draw_bet, reasons);
        }
    }

    fn apply_preflop_discipline_guard(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        let pf_tier = preflop_strength_tier(ctx.bucket);
        if !self.apply_sb_open_discipline(probs, ctx, pf_tier, reasons) {
            self.apply_preflop_facing_raise_discipline(probs, ctx, pf_tier, reasons);
        }
        self.apply_preflop_pair_and_jam_caps(probs, ctx, pf_tier, reasons);
    }

    fn apply_sb_open_discipline(
        &self,
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

        if ctx.to_call == 0 && pf_tier >= 6 {
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
        } else if is_sb_open && pf_tier >= 7 {
            reasons.push("PF_DISCIPLINE_GUARD");
            self.apply_sb_trash_steal_or_fold(probs, ctx, pf_tier);
            true
        } else {
            false
        }
    }

    fn apply_sb_trash_steal_or_fold(&self, probs: &mut [f64], ctx: &GuardContext<'_>, pf_tier: u8) {
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
            || (ctx.is_known_maniac && ctx.bucket >= 114)
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
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                let limp_w = probs[c_i];
                probs[c_i] = 0.0;
                let steal_share = if is_playable_hu_steal && pf_tier == 7 {
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
            if is_playable_hu_steal {
                if let (Some(f_i), Some(r_i)) = (
                    action_idx(ctx.legal_actions, FOLD),
                    first_action_idx(ctx.legal_actions, &[RAISE_MIN, RAISE_THIRD_POT]),
                ) {
                    let fold_shift = if pf_tier == 7 { 0.30 } else { 0.25 };
                    let s = probs[f_i] * fold_shift;
                    probs[f_i] -= s;
                    probs[r_i] += s;
                }
            }
        }
    }

    fn apply_preflop_facing_raise_discipline(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        pf_tier: u8,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.to_call > 0 && pf_tier >= 7 {
            reasons.push("PF_DISCIPLINE_GUARD");
            let mut shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if ctx.pot_odds <= 0.22 || (ctx.pot_odds <= 0.26 && pf_tier <= 7) {
                if let Some(c_i) = first_action_idx(ctx.legal_actions, &[CALL_CHECK, ALL_IN]) {
                    probs[c_i] += shifted;
                }
            } else if ctx.to_call > ctx.bb / 2 && (pf_tier == 8 || ctx.pot_odds > 0.28) {
                if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                    if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                        shifted += probs[c_i];
                        probs[c_i] = 0.0;
                    }
                    probs[f_i] = (probs[f_i] + shifted).max(0.98);
                }
            } else if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                probs[f_i] += shifted;
            }
        } else if ctx.to_call >= ctx.bb * 3 && pf_tier >= 6 {
            reasons.push("PF_DISCIPLINE_GUARD");
            let mut shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if ctx.pot_odds > 0.28 && !ctx.is_extreme_shove_bot {
                if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                    if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                        shifted += probs[c_i];
                        probs[c_i] = 0.0;
                    }
                    probs[f_i] = (probs[f_i] + shifted).max(0.95);
                }
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
            if !nash_ok && pf_tier >= 4 {
                reasons.push("PF_DISCIPLINE_GUARD");
                let mut shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
                if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                    if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                        shifted += probs[c_i];
                        probs[c_i] = 0.0;
                    }
                    probs[f_i] = (probs[f_i] + shifted).max(0.95);
                }
            }
        }
    }

    fn apply_preflop_pair_and_jam_caps(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        pf_tier: u8,
        reasons: &mut Vec<&'static str>,
    ) {
        self.apply_preflop_pair_setmine_guard(probs, ctx, reasons);
        self.apply_preflop_open_jam_cap(probs, ctx, pf_tier);
        self.apply_preflop_deep_3bet_caps(probs, ctx, pf_tier, reasons);
    }

    fn apply_preflop_pair_setmine_guard(
        &self,
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
        if ctx.active_opponents == 1 && ctx.to_call <= ctx.bb * 3 && ctx.pot_odds <= 0.40 {
            if let (Some(f_i), Some(c_i)) = (
                action_idx(ctx.legal_actions, FOLD),
                action_idx(ctx.legal_actions, CALL_CHECK),
            ) {
                if probs[f_i] > 0.0 {
                    probs[c_i] += probs[f_i];
                    probs[f_i] = 0.0;
                }
            }
        }
    }

    fn apply_preflop_open_jam_cap(&self, probs: &mut [f64], ctx: &GuardContext<'_>, pf_tier: u8) {
        if ctx.eff_stack_bb > 20
            && (ctx.to_call <= ctx.bb
                || (ctx.to_call <= ctx.bb * 4 && (ctx.bucket >= 3 || ctx.active_opponents >= 2)))
        {
            if let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN) {
                let s = probs[ai_i];
                if s > 0.0 {
                    probs[ai_i] = 0.0;
                    if let Some(t_i) = first_action_idx(
                        ctx.legal_actions,
                        &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                    ) {
                        probs[t_i] += s;
                    }
                }
            }
        }
        if ctx.eff_stack_bb > 20
            && !ctx.is_allin_call
            && (pf_tier >= 4 || (pf_tier >= 3 && (ctx.is_known_maniac || ctx.is_extreme_shove_bot)))
        {
            if let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN) {
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
    }

    fn apply_preflop_deep_3bet_caps(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        pf_tier: u8,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.eff_stack_bb > 40
            && !ctx.is_allin_call
            && ctx.to_call >= ctx.bb * 6
            && pf_tier == 2
            && ctx.bucket >= 4
        {
            if let Some(ai_i) = action_idx(ctx.legal_actions, ALL_IN) {
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
        }
        if !ctx.is_allin_call
            && ctx.eff_stack_bb >= 25
            && ctx.bucket == 6
            && ctx.to_call >= ctx.bb * 9
            && ctx.my_chips > 0
            && ctx.to_call * 10 >= ctx.my_chips * 3
            && ctx.pot_odds >= 0.35
            && !ctx.is_known_maniac
        {
            if let (Some(c_i), Some(f_i)) = (
                action_idx(ctx.legal_actions, CALL_CHECK),
                action_idx(ctx.legal_actions, FOLD),
            ) {
                if probs[c_i] > 0.0 {
                    reasons.push("PF_DISCIPLINE_GUARD");
                    probs[f_i] += probs[c_i];
                    probs[c_i] = 0.0;
                }
            }
        }
    }

    fn detect_shove_faced(&self, ctx: &GuardContext<'_>) -> (bool, bool) {
        let is_substantial_bet = ctx.to_call >= ctx.bb * 4;
        let is_stack_commit_draw_bet = (ctx.round == 2 || ctx.round == 3)
            && ctx.my_chips > 0
            && ctx.to_call >= ctx.bb * 4
            && ctx.pot_odds >= 0.34
            && (ctx.to_call * 5 >= ctx.my_chips * 2
                || (ctx.my_chips - ctx.to_call) * 3 <= (ctx.pot + ctx.to_call) * 2);
        let is_shove_faced = ctx.pot_odds > 0.18
            && ((ctx.is_allin_call && ctx.to_call >= ctx.bb * 5 && ctx.pot_odds >= 0.28)
                || (is_stack_commit_draw_bet && (15..=23).contains(&ctx.bucket))
                || (is_substantial_bet
                    && (ctx.to_call >= ctx.eff_allin_threshold
                        || (!ctx.can_raise && ctx.to_call >= ctx.bb * 5 && ctx.pot_odds >= 0.28)
                        || (ctx.round == 1 && ctx.to_call >= ctx.bb * 10)
                        || (ctx.round == 4
                            && ctx.to_call >= ctx.bb * 12
                            && ctx.pot_odds >= 0.38)
                        || (ctx.round == 3
                            && ctx.to_call >= ctx.bb * 15
                            && ctx.pot_odds >= 0.34)
                        || (ctx.round == 2 && ctx.to_call >= ctx.bb * 20))));
        (is_shove_faced, is_stack_commit_draw_bet)
    }

    fn apply_shove_defense_fold(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        is_stack_commit_draw_bet: bool,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.is_preflop {
            self.apply_preflop_shove_rescue(probs, ctx);
        }

        let mut is_trash = if ctx.is_preflop {
            self.is_preflop_shove_trash(ctx)
        } else if ctx.round == 4 {
            self.is_river_shove_trash(ctx)
        } else {
            self.is_turn_flop_shove_trash(ctx, is_stack_commit_draw_bet)
        };

        if !ctx.is_preflop && !is_trash && self.is_guard_enabled("RANGE_EQUITY_SHOVE_FOLD") {
            is_trash = self.is_range_equity_shove_trash(ctx, reasons);
        }

        if is_trash && force_action(probs, ctx.legal_actions, FOLD) {
            reasons.push("SHOVE_DEFENSE_FOLD");
        }
    }

    fn apply_preflop_shove_rescue(&self, probs: &mut [f64], ctx: &GuardContext<'_>) {
        let is_multiway = ctx.active_opponents >= 2 && ctx.pot > ctx.to_call + ctx.bb * 3;
        let pf_tier = preflop_strength_tier(ctx.bucket);
        let raw_max_nash_bb = nash_call_shove_max_bb(
            ctx.bucket,
            ctx.active_opponents,
            ctx.is_known_maniac || ctx.is_extreme_shove_bot,
        );
        let max_nash_bb = if is_multiway {
            (raw_max_nash_bb * 2) / 3
        } else {
            raw_max_nash_bb
        };
        let within_nash_rescue = ctx.to_call <= max_nash_bb * ctx.bb
            && ctx.to_call < ctx.bb * 40
            && (pf_tier <= 5 || ctx.is_known_maniac || ctx.is_extreme_shove_bot);

        let should_rescue = if ctx.to_call >= ctx.bb * 40 {
            ctx.bucket <= 1 || (ctx.is_known_maniac && pf_tier <= 2)
        } else {
            (pf_tier <= 1
                || within_nash_rescue
                || (ctx.is_extreme_shove_bot
                    && pf_tier <= 6
                    && ctx.pot_odds <= 0.46
                    && ctx.to_call < ctx.bb * 15))
                && ctx.to_call < ctx.eff_allin_threshold * 2
        };
        if should_rescue {
            if let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
                let f_w = probs[f_i];
                probs[f_i] = 0.0;
                if let Some(c_i) = first_action_idx(ctx.legal_actions, &[CALL_CHECK, ALL_IN]) {
                    probs[c_i] += f_w;
                }
            }
        }
    }

    fn is_preflop_shove_trash(&self, ctx: &GuardContext<'_>) -> bool {
        let is_multiway = ctx.active_opponents >= 2 && ctx.pot > ctx.to_call + ctx.bb * 3;
        let pf_tier = preflop_strength_tier(ctx.bucket);
        let raw_max_call_bb = nash_call_shove_max_bb(
            ctx.bucket,
            ctx.active_opponents,
            ctx.is_known_maniac || ctx.is_extreme_shove_bot,
        );
        let max_call_bb = if is_multiway {
            (raw_max_call_bb * 2) / 3
        } else {
            raw_max_call_bb
        };
        if ctx.pot_odds <= 0.22 || (ctx.pot_odds <= 0.26 && pf_tier <= 7) {
            false
        } else if ctx.to_call >= ctx.bb * 40 {
            if ctx.is_known_maniac {
                pf_tier > 2
            } else if ctx.active_opponents >= 2 {
                ctx.bucket > 1
            } else {
                !(ctx.bucket <= 2 || ctx.bucket == 13)
            }
        } else if ctx.to_call >= ctx.eff_allin_threshold {
            if ctx.is_known_maniac {
                pf_tier > 2
            } else if ctx.active_opponents == 1 {
                pf_tier > 1 && ctx.bucket != 3 && ctx.bucket != 14
            } else {
                pf_tier > 1
            }
        } else if ctx.to_call >= ctx.bb * 12 {
            ctx.to_call > max_call_bb * ctx.bb
        } else if ctx.to_call <= max_call_bb * ctx.bb && pf_tier <= 5 {
            false
        } else if ctx.is_extreme_shove_bot && ctx.pot_odds <= 0.46 {
            pf_tier > 6
        } else if ctx.is_known_maniac {
            pf_tier > 5
        } else {
            pf_tier >= 5
        }
    }

    fn is_river_shove_trash(&self, ctx: &GuardContext<'_>) -> bool {
        let min_river_shove_bucket = if ctx.is_overbet_bluffer {
            if ctx.pot_odds <= 0.26 {
                29
            } else {
                34
            }
        } else if ctx.is_known_maniac {
            40
        } else if ctx.hole.is_some() && ctx.board.is_some() {
            44
        } else {
            52
        };
        ctx.bucket < min_river_shove_bucket
            || (ctx.to_call >= ctx.bb * 25
                && ctx.pot_odds >= 0.35
                && !ctx.is_known_maniac
                && !ctx.is_overbet_bluffer
                && (ctx.is_paired_board || ctx.is_three_flush)
                && ctx.bucket <= 66)
            || (ctx.is_four_flush_board && ctx.bucket <= 79)
            || (ctx.is_four_straight_board && ctx.bucket < 73)
    }

    fn is_turn_flop_shove_trash(
        &self,
        ctx: &GuardContext<'_>,
        is_stack_commit_draw_bet: bool,
    ) -> bool {
        let is_strong_draw = (15..=27).contains(&ctx.bucket);
        let draw_can_call_shove = is_strong_draw
            && if is_stack_commit_draw_bet {
                ((22..=27).contains(&ctx.bucket) && ctx.pot_odds <= 0.34)
                    || ((25..=27).contains(&ctx.bucket) && ctx.pot_odds <= 0.38)
            } else if ctx.round == 3 {
                ((24..=27).contains(&ctx.bucket) && ctx.pot_odds <= 0.31 && !ctx.is_three_flush)
                    || ((ctx.bucket == 26 || ctx.bucket == 27) && ctx.pot_odds <= 0.40)
                    || ((ctx.bucket == 22 || ctx.bucket == 23) && ctx.pot_odds <= 0.22)
            } else {
                let simple_draw_odds_ok = ctx.pot_odds <= 0.30 || ctx.to_call < ctx.bb * 15;
                let premium_draw = ctx.bucket >= 22;
                (!ctx.is_three_flush
                    && (premium_draw || simple_draw_odds_ok)
                    && ctx.pot_odds <= 0.35)
                    || ((ctx.bucket == 22 || ctx.bucket == 23 || ctx.bucket >= 25)
                        && ctx.pot_odds <= 0.35)
            };

        if draw_can_call_shove {
            false
        } else if (ctx.is_known_maniac || ctx.is_overbet_bluffer) && !ctx.is_three_flush {
            let min_b = if ctx.is_overbet_bluffer { 34 } else { 38 };
            ctx.bucket < min_b || (is_stack_commit_draw_bet && (15..=21).contains(&ctx.bucket))
        } else {
            (ctx.is_three_flush && ctx.bucket <= 50)
                || (ctx.round == 3 && ctx.is_paired_board && ctx.bucket < 52)
                || ctx.bucket < 40
                || (is_stack_commit_draw_bet && (15..=21).contains(&ctx.bucket))
                || (ctx.round == 3
                    && (40..=45).contains(&ctx.bucket)
                    && ctx.is_dangerous_board
                    && ctx.pot_odds >= 0.36)
        }
    }

    pub(super) fn is_range_equity_shove_trash(
        &self,
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) -> bool {
        let (Some(h), Some(b)) = (ctx.hole, ctx.board) else {
            return false;
        };
        if b.len() < 3 {
            return false;
        }
        let obs_vpip = if self.total_hands >= 6 {
            self.smoothed_vpip()
        } else {
            0.50
        };
        let heavy_range = ctx.pot_odds >= 0.36
            && ctx.to_call >= ctx.bb * 12
            && !ctx.is_known_maniac
            && !ctx.is_overbet_bluffer;
        let range_eq = calculate_range_equity(h, b, obs_vpip, heavy_range, ctx.active_opponents);
        let eq_margin = if ctx.round == 4 { 0.06 } else { 0.08 };
        if range_eq + eq_margin < ctx.pot_odds && ctx.bucket < 65 {
            reasons.push("RANGE_EQUITY_SHOVE_FOLD");
            true
        } else {
            false
        }
    }
}
