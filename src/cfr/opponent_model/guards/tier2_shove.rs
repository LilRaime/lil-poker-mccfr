/*! Priority Tier 2A: All-In / Heavy-Bet Shove Detection and Defense */
use super::super::tracker::OpponentTracker;
use super::context::GuardContext;
use super::guard_id::{GuardId, GuardSet};
use super::helpers::{action_idx, first_action_idx, force_action};
use crate::cfr::abstraction::{
    BUCKET_FLUSH_MIN, BUCKET_OVERPAIR_MIN, BUCKET_STRAIGHT_MIN, BUCKET_TOP_PAIR_MIN,
    BUCKET_TRIPS_MIN, PostflopBucket, calculate_range_equity, nash_call_shove_max_bb,
    preflop_strength_tier,
};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER, ROUND_TURN,
};

impl OpponentTracker {
    pub(super) fn apply_tier2_preflop_and_shove_defense(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut GuardSet,
    ) {
        if ctx.is_preflop && self.is_guard_enabled(GuardId::PfDisciplineGuard) {
            Self::apply_preflop_discipline_guard(probs, ctx, reasons);
        }

        let (is_shove_faced, is_stack_commit_draw_bet) = Self::detect_shove_faced(ctx);
        if is_shove_faced && self.is_guard_enabled(GuardId::ShoveDefenseFold) {
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
                    && ctx
                        .post_bucket
                        .in_range(crate::cfr::abstraction::BUCKET_STRONG_DRAW_MIN as u8, 23))
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
                            && (ctx.post_bucket.is_unpaired()
                                || ctx.post_bucket.is_bare_weak_underpair())
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
        reasons: &mut GuardSet,
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

        if !ctx.is_preflop && !is_trash && self.is_guard_enabled(GuardId::RangeEquityShoveFold) {
            is_trash = self.is_range_equity_shove_trash(ctx, reasons);
        }

        if is_trash && force_action(probs, ctx.legal_actions, FOLD) {
            reasons.insert(GuardId::ShoveDefenseFold);
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
            let rem = ctx.my_chips.saturating_sub(ctx.to_call);
            let new_pot = ctx.pot + ctx.to_call;
            let prefers_shove = !ctx.is_allin_call
                && ctx.can_raise
                && ctx.my_chips > ctx.to_call
                && (rem * 20 < new_pot * 11
                    || (ctx.eff_stack_bb <= 25 && ctx.to_call * 5 >= ctx.my_chips * 2));
            let order: &[u8] = if prefers_shove {
                &[ALL_IN, CALL_CHECK]
            } else {
                &[CALL_CHECK, ALL_IN]
            };
            if let Some(t_i) = first_action_idx(ctx.legal_actions, order) {
                probs[t_i] += f_w;
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
            if ctx.pot_odds <= 0.26 {
                PostflopBucket::THREE_FLUSH_BOTTOM_PAIR as usize
            } else {
                34
            }
        } else if ctx.is_known_maniac {
            BUCKET_TOP_PAIR_MIN
        } else if ctx.hole.is_some() && ctx.board.is_some() {
            if is_dry_short_spr_river_bet {
                PostflopBucket::CLEAN_MID_PAIR_MIN as usize
            } else {
                PostflopBucket::DRY_WEAK_TOP_PAIR as usize
            }
        } else {
            PostflopBucket::HIGH_VULNERABLE_OVERPAIR as usize
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
        let pb = ctx.post_bucket;
        let is_strong_draw = pb.is_strong_draw();
        let draw_can_call_shove = is_strong_draw
            && if is_stack_commit_draw_bet {
                (pb.is_nut_or_combo_draw() && ctx.pot_odds <= 0.34)
                    || (pb.in_range(25, 27) && ctx.pot_odds <= 0.38)
            } else if ctx.round == ROUND_TURN {
                (pb.is_combo_draw() && ctx.pot_odds <= 0.31 && !ctx.is_three_flush)
                    || (pb.is_nut_combo_or_pair_nfd() && ctx.pot_odds <= 0.40)
                    || (pb.is_bare_nut_flush_draw() && ctx.pot_odds <= 0.22)
            } else {
                let simple_draw_odds_ok = ctx.pot_odds <= 0.30 || ctx.to_call < ctx.bb * 15;
                let premium_draw = pb.as_u8() >= PostflopBucket::NUT_DRAW_MIN;
                (!ctx.is_three_flush
                    && (premium_draw || simple_draw_odds_ok)
                    && ctx.pot_odds <= 0.35)
                    || ((pb.is_bare_nut_flush_draw() || pb.as_u8() >= 25) && ctx.pot_odds <= 0.35)
            };

        if draw_can_call_shove {
            false
        } else if (ctx.is_known_maniac || ctx.is_overbet_bluffer) && !ctx.is_three_flush {
            let min_b = if ctx.is_overbet_bluffer {
                if ctx.round == ROUND_FLOP {
                    if ctx.to_call >= ctx.bb * 25 && ctx.pot_odds >= 0.36 {
                        37
                    } else {
                        PostflopBucket::DRY_WEAK_BOTTOM_PAIR_MAX as usize
                    }
                } else if ctx.to_call >= ctx.bb * 28
                    && ctx.pot_odds >= 0.36
                    && !pb.has_pair_redraw()
                {
                    BUCKET_TOP_PAIR_MIN
                } else {
                    34
                }
            } else {
                PostflopBucket::MID_PAIR_REDRAW as usize
            };
            ctx.bucket < min_b
                || (is_stack_commit_draw_bet && pb.is_non_nut_draw())
                || (ctx.round == ROUND_TURN
                    && ctx.is_paired_board
                    && ctx.bucket < BUCKET_OVERPAIR_MIN
                    && ctx.to_call >= ctx.bb * 15
                    && !ctx.is_overbet_bluffer)
        } else {
            (ctx.is_three_flush && ctx.bucket <= BUCKET_OVERPAIR_MIN)
                || (ctx.round == ROUND_TURN
                    && ctx.is_paired_board
                    && pb.as_u8() < PostflopBucket::HIGH_VULNERABLE_OVERPAIR)
                || ctx.bucket < BUCKET_TOP_PAIR_MIN
                || (is_stack_commit_draw_bet && pb.is_non_nut_draw())
                || (ctx.round == ROUND_TURN
                    && pb.in_range(
                        BUCKET_TOP_PAIR_MIN as u8,
                        PostflopBucket::STRONG_DRY_TOP_PAIR_MIN,
                    )
                    && ctx.is_dangerous_board
                    && ctx.pot_odds >= 0.36)
        }
    }

    pub(super) fn is_range_equity_shove_trash(
        &self,
        ctx: &GuardContext<'_>,
        reasons: &mut GuardSet,
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
            reasons.insert(GuardId::RangeEquityShoveFold);
            true
        } else {
            false
        }
    }
}
