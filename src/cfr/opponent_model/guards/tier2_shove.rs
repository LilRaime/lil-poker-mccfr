/*! Priority Tier 2A: All-In / Heavy-Bet Shove Detection and Defense */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::helpers::{action_idx, first_action_idx, force_action};
use crate::cfr::abstraction::{
    BUCKET_FLUSH_MIN, BUCKET_OVERPAIR_MIN, BUCKET_STRAIGHT_MIN, BUCKET_STRONG_DRAW_MIN,
    BUCKET_TOP_PAIR_MIN, BUCKET_TRIPS_MIN, BUCKET_WEAK_PAIR_MIN, calculate_range_equity,
    nash_call_shove_max_bb, preflop_strength_tier,
};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER, ROUND_TURN,
};

impl OpponentTracker {
    pub(super) fn apply_tier2_preflop_and_shove_defense(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut Vec<&'static str>,
    ) {
        if ctx.is_preflop && self.is_guard_enabled("PF_DISCIPLINE_GUARD") {
            Self::apply_preflop_discipline_guard(probs, ctx, reasons);
        }

        let (is_shove_faced, is_stack_commit_draw_bet) = Self::detect_shove_faced(ctx);
        if is_shove_faced && self.is_guard_enabled("SHOVE_DEFENSE_FOLD") {
            self.apply_shove_defense_fold(probs, ctx, is_stack_commit_draw_bet, reasons);
        }
    }

    fn detect_shove_faced(ctx: &GuardContext<'_>) -> (bool, bool) {
        let is_substantial_bet = ctx.to_call >= ctx.bb * 4;
        let is_stack_commit_draw_bet = (ctx.round == ROUND_FLOP || ctx.round == ROUND_TURN)
            && ctx.my_chips > 0
            && ctx.to_call >= ctx.bb * 4
            && ctx.pot_odds >= 0.34
            && (ctx.to_call * 5 >= ctx.my_chips * 2
                || (ctx.my_chips - ctx.to_call) * 3 <= (ctx.pot + ctx.to_call) * 2);
        let is_shove_faced = ctx.pot_odds > 0.18
            && ((ctx.is_allin_call
                && ctx.to_call >= ctx.bb * 5
                && (ctx.pot_odds >= 0.28 || (ctx.round >= ROUND_TURN && ctx.pot_odds >= 0.22)))
                || (is_stack_commit_draw_bet
                    && (BUCKET_STRONG_DRAW_MIN..=23).contains(&ctx.bucket))
                || (is_substantial_bet
                    && (ctx.to_call >= ctx.eff_allin_threshold
                        || (!ctx.can_raise && ctx.to_call >= ctx.bb * 5 && ctx.pot_odds >= 0.28)
                        || (ctx.round == ROUND_PREFLOP && ctx.to_call >= ctx.bb * 10)
                        || (ctx.round == ROUND_RIVER
                            && ctx.to_call >= ctx.bb * 12
                            && ctx.pot_odds >= 0.30)
                        || (ctx.round == ROUND_TURN
                            && ctx.to_call >= ctx.bb * 12
                            && ctx.pot_odds >= 0.30)
                        || (ctx.round == ROUND_TURN
                            && ctx.is_three_flush
                            && ctx.is_paired_board
                            && ctx.bucket <= 34
                            && ctx.bucket != 33
                            && ctx.to_call * 2 >= ctx.bb * 7
                            && ctx.pot_odds >= 0.24)
                        || (ctx.round == ROUND_FLOP && ctx.to_call >= ctx.bb * 20))));
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
            Self::apply_preflop_shove_rescue(probs, ctx);
        }

        let mut is_trash = if ctx.is_preflop {
            Self::is_preflop_shove_trash(ctx)
        } else if ctx.round == ROUND_RIVER {
            Self::is_river_shove_trash(ctx)
        } else {
            Self::is_turn_flop_shove_trash(ctx, is_stack_commit_draw_bet)
        };

        if !ctx.is_preflop && !is_trash && self.is_guard_enabled("RANGE_EQUITY_SHOVE_FOLD") {
            is_trash = self.is_range_equity_shove_trash(ctx, reasons);
        }

        if is_trash && force_action(probs, ctx.legal_actions, FOLD) {
            reasons.push("SHOVE_DEFENSE_FOLD");
        }
    }

    fn apply_preflop_shove_rescue(probs: &mut [f64], ctx: &GuardContext<'_>) {
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
        let is_genuine_shove_or_short = ctx.is_allin_call
            || !ctx.can_raise
            || ctx.eff_stack_bb <= 18
            || (ctx.my_chips > 0 && ctx.to_call * 2 >= ctx.my_chips);
        let within_nash_rescue = ctx.to_call <= max_nash_bb * ctx.bb
            && ctx.to_call < ctx.bb * 40
            && (pf_tier <= 3
                || (is_genuine_shove_or_short
                    && (pf_tier <= 5 || ctx.is_known_maniac || ctx.is_extreme_shove_bot)));

        let should_rescue = if ctx.to_call >= ctx.bb * 40 {
            ctx.bucket <= 1 || (ctx.is_known_maniac && pf_tier <= 2)
        } else {
            (pf_tier <= 1
                || within_nash_rescue
                || (is_genuine_shove_or_short
                    && ctx.is_extreme_shove_bot
                    && pf_tier <= 6
                    && ctx.pot_odds <= 0.46
                    && ctx.to_call < ctx.bb * 15))
                && ctx.to_call < ctx.eff_allin_threshold * 2
        };
        if should_rescue && let Some(f_i) = action_idx(ctx.legal_actions, FOLD) {
            let f_w = probs[f_i];
            probs[f_i] = 0.0;
            if let Some(c_i) = first_action_idx(ctx.legal_actions, &[CALL_CHECK, ALL_IN]) {
                probs[c_i] += f_w;
            }
        }
    }

    fn is_preflop_shove_trash(ctx: &GuardContext<'_>) -> bool {
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

    fn is_river_shove_trash(ctx: &GuardContext<'_>) -> bool {
        let pre_pot = (ctx.pot - ctx.to_call).max(1);
        let is_dry_short_spr_river_bet = ctx.to_call < ctx.bb * 10
            && ctx.pot_odds <= 0.33
            && ctx.to_call <= pre_pot
            && !ctx.is_paired_board
            && !ctx.is_three_flush
            && !ctx.is_dangerous_board
            && !ctx.is_four_board_danger;
        let min_river_shove_bucket = if ctx.is_overbet_bluffer {
            if ctx.pot_odds <= 0.26 { 29 } else { 34 }
        } else if ctx.is_known_maniac {
            BUCKET_TOP_PAIR_MIN
        } else if ctx.hole.is_some() && ctx.board.is_some() {
            if is_dry_short_spr_river_bet { 35 } else { 44 }
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
            || (ctx.is_four_flush_board && ctx.bucket <= BUCKET_FLUSH_MIN)
            || (ctx.is_four_straight_board && ctx.bucket < BUCKET_STRAIGHT_MIN)
    }

    fn is_turn_flop_shove_trash(ctx: &GuardContext<'_>, is_stack_commit_draw_bet: bool) -> bool {
        let is_strong_draw = (BUCKET_STRONG_DRAW_MIN..BUCKET_WEAK_PAIR_MIN).contains(&ctx.bucket);
        let draw_can_call_shove = is_strong_draw
            && if is_stack_commit_draw_bet {
                ((22..BUCKET_WEAK_PAIR_MIN).contains(&ctx.bucket) && ctx.pot_odds <= 0.34)
                    || ((25..BUCKET_WEAK_PAIR_MIN).contains(&ctx.bucket) && ctx.pot_odds <= 0.38)
            } else if ctx.round == ROUND_TURN {
                ((24..BUCKET_WEAK_PAIR_MIN).contains(&ctx.bucket)
                    && ctx.pot_odds <= 0.31
                    && !ctx.is_three_flush)
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
            let min_b = if ctx.is_overbet_bluffer {
                if ctx.round == ROUND_FLOP { 30 } else { 34 }
            } else {
                38
            };
            ctx.bucket < min_b
                || (is_stack_commit_draw_bet && (BUCKET_STRONG_DRAW_MIN..=21).contains(&ctx.bucket))
                || (ctx.round == ROUND_TURN
                    && ctx.is_paired_board
                    && ctx.bucket < BUCKET_OVERPAIR_MIN
                    && ctx.to_call >= ctx.bb * 15
                    && !ctx.is_overbet_bluffer)
        } else {
            (ctx.is_three_flush && ctx.bucket <= BUCKET_OVERPAIR_MIN)
                || (ctx.round == ROUND_TURN && ctx.is_paired_board && ctx.bucket < 52)
                || ctx.bucket < BUCKET_TOP_PAIR_MIN
                || (is_stack_commit_draw_bet && (BUCKET_STRONG_DRAW_MIN..=21).contains(&ctx.bucket))
                || (ctx.round == ROUND_TURN
                    && (BUCKET_TOP_PAIR_MIN..=45).contains(&ctx.bucket)
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
        let eq_margin = if ctx.round == ROUND_RIVER { 0.06 } else { 0.08 };
        if range_eq + eq_margin < ctx.pot_odds && ctx.bucket < BUCKET_TRIPS_MIN {
            reasons.push("RANGE_EQUITY_SHOVE_FOLD");
            true
        } else {
            false
        }
    }
}
