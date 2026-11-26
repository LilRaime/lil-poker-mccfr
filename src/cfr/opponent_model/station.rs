/*! Anti-Passive / Calling-Station Value-Extraction Boost & Postflop Barrel Defense */
use super::guards::{
    GuardContext, GuardId, GuardSet, action_idx, first_action_idx, force_action,
    is_turn_commit_underpair_fold, is_turn_paired_or_flush_underpair_fold,
};
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::{
    BUCKET_FLUSH_MIN, BUCKET_STRONG_DRAW_MIN, BUCKET_TRIPS_MIN, BUCKET_WEAK_PAIR_MIN,
    PostflopBucket, preflop_strength_tier,
};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, ROUND_FLOP, ROUND_RIVER,
    ROUND_TURN,
};

impl OpponentTracker {
    /** Checks if opponent predominantly checks and calls without raising */
    pub fn is_passive_station(&self) -> bool {
        if !self.exploit_enabled {
            return false;
        }
        let overall_station = self.total_actions() >= 8
            && self.raise_ratio() < 0.12
            && (self.call_ratio() > 0.50 || self.fold_ratio() < 0.22);
        let post_total: u64 = self.flop_actions.iter().sum::<u64>()
            + self.turn_actions.iter().sum::<u64>()
            + self.river_actions.iter().sum::<u64>();
        let postflop_station = self.total_hands >= 8
            && post_total >= 10
            && self.vpip_ratio() >= 0.65
            && self.pfr_ratio() <= 0.30
            && self.postflop_raise_rate() < 0.15
            && self.fold_ratio() < 0.30;
        overall_station || postflop_station
    }

    /** Shift passive check/min-bet weight directly into geometric value sizing (`RAISE_HALF_POT` / `RAISE_THIRD_POT`)
     * so the bot extracts full stacks in 15-25 hands instead of checking down Top Pair / Overpair / Two Pair! */
    pub(super) fn apply_passive_station_boost(probs: &mut [f64], ctx: &GuardContext<'_>) {
        if ctx.is_preflop && ctx.to_call <= ctx.bb {
            Self::boost_station_preflop(probs, ctx);
        } else if !ctx.is_preflop && ctx.to_call == 0 && !ctx.is_four_board_danger {
            Self::boost_station_postflop(probs, ctx);
        }
    }

    fn boost_station_preflop(probs: &mut [f64], ctx: &GuardContext<'_>) {
        let pf_tier = preflop_strength_tier(ctx.bucket);
        if pf_tier <= 3 {
            if let Some(t_i) = first_action_idx(
                ctx.legal_actions,
                &[RAISE_HALF_POT, RAISE_THIRD_POT, RAISE_MIN],
            ) {
                let mut shifted = 0.0f64;
                for (i, &a) in ctx.legal_actions.iter().enumerate() {
                    if a == CALL_CHECK {
                        let s = probs[i] * 0.98;
                        probs[i] -= s;
                        shifted += s;
                    } else if a == RAISE_MIN && i != t_i {
                        let s = probs[i] * 0.90;
                        probs[i] -= s;
                        shifted += s;
                    } else if a == RAISE_THIRD_POT && i != t_i {
                        let s = probs[i] * 0.50;
                        probs[i] -= s;
                        shifted += s;
                    }
                }
                probs[t_i] = (probs[t_i] + shifted).max(0.80);
            }
        } else if pf_tier <= 5
            && ctx.bucket <= 105
            && let Some(t_i) = first_action_idx(
                ctx.legal_actions,
                &[RAISE_THIRD_POT, RAISE_HALF_POT, RAISE_MIN],
            )
        {
            let mut shifted = 0.0f64;
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                let s = probs[c_i] * 0.85;
                probs[c_i] -= s;
                shifted += s;
            }
            if let Some(m_i) = action_idx(ctx.legal_actions, RAISE_MIN)
                && m_i != t_i
            {
                let s = probs[m_i] * 0.70;
                probs[m_i] -= s;
                shifted += s;
            }
            probs[t_i] += shifted;
        }
    }

    fn boost_station_postflop(probs: &mut [f64], ctx: &GuardContext<'_>) {
        let is_dangerous_river = ctx.round == ROUND_RIVER
            && (ctx.is_dangerous_board || ctx.is_three_flush || ctx.is_three_straight_board);

        let pb = ctx.post_bucket;
        if pb.is_air_or_weak_draw() {
            /* Pure air against a station who never folds: stop burning chips on air bluffs! */
            let mut shifted = 0.0f64;
            for (i, &a) in ctx.legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    let s = probs[i] * 0.90;
                    probs[i] -= s;
                    shifted += s;
                }
            }
            if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
                probs[c_i] += shifted;
            }
        } else if ctx.bucket >= BUCKET_TRIPS_MIN
            && !pb.is_board_chop_or_idiot_straight()
            && !(ctx.is_three_flush && ctx.bucket < BUCKET_FLUSH_MIN)
            && !(ctx.round == ROUND_RIVER && pb.is_non_idiot_straight() && ctx.is_paired_board)
        {
            Self::boost_station_monsters(probs, ctx);
        } else if pb.is_two_pair() && !ctx.is_three_flush {
            Self::boost_station_two_pair(probs, ctx, is_dangerous_river);
        } else if pb.is_strong_top_pair_or_overpair() && !ctx.is_three_flush {
            Self::boost_station_top_pair(probs, ctx, is_dangerous_river);
        } else if pb.in_range(
            PostflopBucket::CLEAN_MID_PAIR_MIN,
            PostflopBucket::DRY_WEAK_TOP_PAIR,
        ) && !pb.is_counterfeited_top_pair()
            && pb.as_u8() != PostflopBucket::FOUR_DANGER_TOP_PAIR
            && !ctx.is_three_flush
            && !is_dangerous_river
        {
            Self::boost_station_thin_value(probs, ctx);
        }
    }

    fn boost_station_monsters(probs: &mut [f64], ctx: &GuardContext<'_>) {
        /* Trips / Set / Straight / Flush / Full House+: relentless heavy value */
        let hp_idx = action_idx(ctx.legal_actions, RAISE_HALF_POT);
        let ai_idx = action_idx(ctx.legal_actions, ALL_IN);
        let tp_idx = action_idx(ctx.legal_actions, RAISE_THIRD_POT);
        let mut shifted = 0.0f64;
        for (i, &a) in ctx.legal_actions.iter().enumerate() {
            if a == CALL_CHECK {
                let s = probs[i] * 0.98;
                probs[i] -= s;
                shifted += s;
            } else if a == RAISE_MIN {
                let s = probs[i] * 0.95;
                probs[i] -= s;
                shifted += s;
            } else if a == RAISE_THIRD_POT && (hp_idx.is_some() || ai_idx.is_some()) {
                let s = probs[i] * 0.65;
                probs[i] -= s;
                shifted += s;
            }
        }
        if let Some(hp_i) = hp_idx {
            if let Some(ai_i) = ai_idx {
                probs[hp_i] = (probs[hp_i] + shifted * 0.70).max(0.65);
                probs[ai_i] += shifted * 0.30;
            } else {
                probs[hp_i] = (probs[hp_i] + shifted).max(0.85);
            }
        } else if let Some(tp_i) = tp_idx {
            probs[tp_i] = (probs[tp_i] + shifted).max(0.85);
        }
    }

    fn boost_station_two_pair(probs: &mut [f64], ctx: &GuardContext<'_>, is_dangerous_river: bool) {
        /* Two Pair: barrel large value (RAISE_HALF_POT / RAISE_THIRD_POT) */
        let target_idx =
            if is_dangerous_river && ctx.post_bucket.as_u8() < PostflopBucket::CLEAN_TWO_PAIR_MIN {
                action_idx(ctx.legal_actions, RAISE_THIRD_POT)
            } else {
                first_action_idx(ctx.legal_actions, &[RAISE_HALF_POT, RAISE_THIRD_POT])
            };
        if let Some(t_i) = target_idx {
            let mut shifted = 0.0f64;
            for (i, &a) in ctx.legal_actions.iter().enumerate() {
                if a == CALL_CHECK {
                    let s = probs[i] * 0.98;
                    probs[i] -= s;
                    shifted += s;
                } else if a == RAISE_MIN && i != t_i {
                    let s = probs[i] * 0.90;
                    probs[i] -= s;
                    shifted += s;
                } else if a == RAISE_THIRD_POT && i != t_i {
                    let s = probs[i] * 0.55;
                    probs[i] -= s;
                    shifted += s;
                }
            }
            probs[t_i] = (probs[t_i] + shifted).max(0.80);
        }
    }

    fn boost_station_top_pair(probs: &mut [f64], ctx: &GuardContext<'_>, is_dangerous_river: bool) {
        /* Top Pair Good/Top Kicker or Overpair: extract multi-street value instead of checking behind! */
        let mut shifted = 0.0f64;
        if let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK) {
            let s = probs[c_i] * 0.98;
            probs[c_i] -= s;
            shifted += s;
        }
        if let Some(m_i) = action_idx(ctx.legal_actions, RAISE_MIN) {
            let s = probs[m_i] * 0.85;
            probs[m_i] -= s;
            shifted += s;
        }
        let hp_idx = action_idx(ctx.legal_actions, RAISE_HALF_POT);
        let tp_idx = action_idx(ctx.legal_actions, RAISE_THIRD_POT);
        if !is_dangerous_river {
            if let Some(hp_i) = hp_idx {
                if let Some(tp_i) = tp_idx {
                    probs[hp_i] = (probs[hp_i] + shifted * 0.65).max(0.60);
                    probs[tp_i] += shifted * 0.35;
                } else {
                    probs[hp_i] = (probs[hp_i] + shifted).max(0.80);
                }
            } else if let Some(tp_i) = tp_idx {
                probs[tp_i] = (probs[tp_i] + shifted).max(0.80);
            }
        } else if let Some(tp_i) = tp_idx {
            probs[tp_i] = (probs[tp_i] + shifted).max(0.75);
        }
    }

    fn boost_station_thin_value(probs: &mut [f64], ctx: &GuardContext<'_>) {
        /* Top Pair Weak Kicker or Strong Middle Pair: consistent thin value (33% pot) */
        if let Some(t_i) = first_action_idx(ctx.legal_actions, &[RAISE_THIRD_POT, RAISE_MIN])
            && let Some(c_i) = action_idx(ctx.legal_actions, CALL_CHECK)
        {
            let s = probs[c_i] * 0.68;
            probs[c_i] -= s;
            probs[t_i] = (probs[t_i] + s).max(0.60);
        }
    }

    /** Postflop Heavy-Bet / Multi-Street Barrel Defense
     * & Unconditional Pot-Commitment Rescue (Priority Tier 3) */
    pub(super) fn apply_postflop_barrel_and_pot_commitment_guards(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        reasons: &mut GuardSet,
    ) {
        let pb = ctx.post_bucket;
        if self.is_guard_enabled(GuardId::PostBarrelFold) {
            let is_weak_underpair = pb.is_bare_weak_underpair();
            let fold_three_flush_air = !ctx.is_preflop
                && ctx.is_three_flush
                && ctx.to_call >= ctx.bb
                && ctx.pot_odds >= 0.18
                && ctx.bucket <= 18
                && !(ctx.has_turn_one_card_fd
                    && (ctx.pot_odds <= 0.28 || (ctx.is_overbet_bluffer && ctx.pot_odds <= 0.34)));
            if fold_three_flush_air && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.insert(GuardId::PostBarrelFold);
            }
            let fold_flop_weak_pair = ctx.round == ROUND_FLOP
                && !ctx.is_overbet_bluffer
                && ctx.pot_odds >= 0.28
                && ((ctx.to_call >= ctx.bb * 4 && pb.is_weak_bottom_pair())
                    || (ctx.pot >= ctx.bb * 10 && ctx.to_call >= ctx.bb * 6 && is_weak_underpair));
            if fold_flop_weak_pair && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.insert(GuardId::PostBarrelFold);
            }
            if is_turn_paired_or_flush_underpair_fold(ctx)
                && force_action(probs, ctx.legal_actions, FOLD)
            {
                reasons.insert(GuardId::PostBarrelFold);
            }
            if is_turn_commit_underpair_fold(ctx) && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.insert(GuardId::PostBarrelFold);
            }
            if ctx.round >= ROUND_TURN && ctx.to_call >= ctx.bb * 5 {
                let should_fold_weak_pair =
                    pb.is_weak_bottom_pair() && !ctx.is_overbet_bluffer && ctx.pot_odds >= 0.33;
                let should_fold_three_flush_tp = ctx.is_three_flush
                    && pb.in_range(BUCKET_WEAK_PAIR_MIN as u8, 42)
                    && ctx.pot_odds >= 0.28;
                if (should_fold_weak_pair || should_fold_three_flush_tp)
                    && force_action(probs, ctx.legal_actions, FOLD)
                {
                    reasons.insert(GuardId::PostBarrelFold);
                }
            }
        }

        let flop_opp_pot = (ctx.pot - ctx.to_call).max(1);
        let heavy_flop_shove = ctx.to_call >= ctx.bb * 25 && ctx.pot_odds >= 0.36;
        let min_flop_draw_b = if heavy_flop_shove {
            25
        } else {
            BUCKET_STRONG_DRAW_MIN
        };
        let min_flop_catch_b = if heavy_flop_shove { 37 } else { 29 };
        let is_flop_overbet_or_large = (ctx.to_call * 10 > flop_opp_pot * 12
            && ((min_flop_draw_b..BUCKET_WEAK_PAIR_MIN).contains(&ctx.bucket)
                || ctx.bucket >= min_flop_catch_b))
            || (ctx.to_call * 10 >= flop_opp_pot * 7 && ctx.bucket >= min_flop_catch_b.max(31));
        if ctx.round == ROUND_FLOP
            && ctx.is_overbet_bluffer
            && ctx.to_call > 0
            && !ctx.is_three_flush
            && !ctx.is_four_board_danger
            && is_flop_overbet_or_large
            && self.is_guard_enabled(GuardId::OverbetBluffCatch)
            && let (Some(f_i), Some(c_i)) = (
                action_idx(ctx.legal_actions, FOLD),
                action_idx(ctx.legal_actions, CALL_CHECK),
            )
            && probs[f_i] > 0.30
        {
            reasons.insert(GuardId::OverbetBluffCatch);
            let s = probs[f_i] * 0.65;
            probs[f_i] -= s;
            probs[c_i] += s;
        }

        /* Final Unconditional Pot-Commitment Rescue */
        if ctx.to_call > 0
            && ctx.allow_pot_commit_rescue
            && (ctx.pot_odds <= 0.12
                || (ctx.pot_odds <= 0.15
                    && (ctx.is_preflop || ctx.bucket >= BUCKET_STRONG_DRAW_MIN))
                || ctx.never_fold_postflop)
            && self.is_guard_enabled(GuardId::PotCommitTerminal)
            && let Some(f_i) = action_idx(ctx.legal_actions, FOLD)
        {
            let f_prob = probs[f_i];
            if f_prob > 0.0 {
                reasons.insert(GuardId::PotCommitTerminal);
                probs[f_i] = 0.0;
                if let Some(c_i) = ctx
                    .legal_actions
                    .iter()
                    .position(|&a| a == CALL_CHECK || a == ALL_IN)
                {
                    probs[c_i] += f_prob;
                }
            }
        }
    }
}
