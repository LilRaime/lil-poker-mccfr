/*! Anti-Passive / Calling-Station Value-Extraction Boost & Postflop Barrel Defense */
use super::guards::{GuardContext, action_idx, first_action_idx, force_action};
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::{
    BUCKET_FLUSH_MIN, BUCKET_STRAIGHT_MAX, BUCKET_STRAIGHT_MIN, BUCKET_STRONG_DRAW_MIN,
    BUCKET_TOP_PAIR_MIN, BUCKET_TRIPS_MIN, BUCKET_TWO_PAIR_MIN, BUCKET_WEAK_PAIR_MIN,
    preflop_strength_tier,
};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, ROUND_FLOP, ROUND_RIVER,
    ROUND_TURN,
};

impl OpponentTracker {
    /** Checks if opponent predominantly checks and calls without raising */
    pub fn is_passive_station(&self) -> bool {
        self.exploit_enabled
            && self.total_actions() >= 8
            && self.raise_ratio() < 0.12
            && (self.call_ratio() > 0.50 || self.fold_ratio() < 0.22)
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

        if ctx.bucket < BUCKET_STRONG_DRAW_MIN {
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
            && ctx.bucket != BUCKET_STRAIGHT_MIN
            && !(ctx.is_three_flush && ctx.bucket < BUCKET_FLUSH_MIN)
            && !(ctx.round == ROUND_RIVER
                && (74..=BUCKET_STRAIGHT_MAX).contains(&ctx.bucket)
                && ctx.is_paired_board)
        {
            Self::boost_station_monsters(probs, ctx);
        } else if (BUCKET_TWO_PAIR_MIN..BUCKET_TRIPS_MIN).contains(&ctx.bucket)
            && !ctx.is_three_flush
        {
            Self::boost_station_two_pair(probs, ctx, is_dangerous_river);
        } else if (45..=56).contains(&ctx.bucket) && !ctx.is_three_flush {
            Self::boost_station_top_pair(probs, ctx, is_dangerous_river);
        } else if (35..=44).contains(&ctx.bucket)
            && ctx.bucket != 39
            && ctx.bucket != BUCKET_TOP_PAIR_MIN
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
        let target_idx = if is_dangerous_river && ctx.bucket < 61 {
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
        reasons: &mut Vec<&'static str>,
    ) {
        if self.is_guard_enabled("POST_BARREL_FOLD") {
            let is_weak_underpair =
                (BUCKET_WEAK_PAIR_MIN..=34).contains(&ctx.bucket) && ctx.bucket != 33;
            let fold_three_flush_air = !ctx.is_preflop
                && ctx.is_three_flush
                && ctx.to_call >= ctx.bb
                && ctx.pot_odds >= 0.18
                && ctx.bucket <= 18;
            if fold_three_flush_air && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.push("POST_BARREL_FOLD");
            }
            let fold_flop_weak_pair = ctx.round == ROUND_FLOP
                && !ctx.is_overbet_bluffer
                && ctx.pot_odds >= 0.28
                && ((ctx.to_call >= ctx.bb * 4
                    && (BUCKET_WEAK_PAIR_MIN..=30).contains(&ctx.bucket))
                    || (ctx.pot >= ctx.bb * 10 && ctx.to_call >= ctx.bb * 6 && is_weak_underpair));
            if fold_flop_weak_pair && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.push("POST_BARREL_FOLD");
            }
            let fold_turn_paired_or_flush_underpair = ctx.round == ROUND_TURN
                && is_weak_underpair
                && !ctx.is_overbet_bluffer
                && ((ctx.is_paired_board
                    && ctx.is_three_flush
                    && ctx.to_call * 2 >= ctx.bb * 7
                    && ctx.pot_odds >= 0.24)
                    || ((ctx.is_paired_board || ctx.is_three_flush)
                        && ctx.to_call * 2 >= ctx.bb * 7
                        && ctx.pot_odds >= 0.25
                        && (ctx.to_call >= ctx.bb * 5
                            || (ctx.my_chips > 0 && ctx.to_call * 5 >= ctx.my_chips * 2))));
            if fold_turn_paired_or_flush_underpair && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.push("POST_BARREL_FOLD");
            }
            let is_turn_bare_underpair = (BUCKET_WEAK_PAIR_MIN..BUCKET_TOP_PAIR_MIN)
                .contains(&ctx.bucket)
                && ctx.bucket != 33
                && ctx.bucket != 38;
            let fold_turn_commit_underpair = ctx.round == ROUND_TURN
                && is_turn_bare_underpair
                && !ctx.is_overbet_bluffer
                && !ctx.is_known_maniac
                && ctx.to_call >= ctx.bb * 15
                && ctx.pot_odds >= 0.25
                && ctx.my_chips > 0
                && ctx.to_call * 2 >= ctx.my_chips;
            if fold_turn_commit_underpair && force_action(probs, ctx.legal_actions, FOLD) {
                reasons.push("POST_BARREL_FOLD");
            }
            if ctx.round >= ROUND_TURN && ctx.to_call >= ctx.bb * 5 {
                let should_fold_weak_pair =
                    (BUCKET_WEAK_PAIR_MIN..=30).contains(&ctx.bucket) && ctx.pot_odds >= 0.33;
                let should_fold_three_flush_tp = ctx.is_three_flush
                    && (BUCKET_WEAK_PAIR_MIN..=42).contains(&ctx.bucket)
                    && ctx.pot_odds >= 0.28;
                if (should_fold_weak_pair || should_fold_three_flush_tp)
                    && force_action(probs, ctx.legal_actions, FOLD)
                {
                    reasons.push("POST_BARREL_FOLD");
                }
            }
        }

        let flop_opp_pot = (ctx.pot - ctx.to_call).max(1);
        let is_flop_overbet_or_large = (ctx.to_call * 10 > flop_opp_pot * 12
            && ((BUCKET_STRONG_DRAW_MIN..BUCKET_WEAK_PAIR_MIN).contains(&ctx.bucket)
                || ctx.bucket >= 29))
            || (ctx.to_call * 10 >= flop_opp_pot * 7 && ctx.bucket >= 31);
        if ctx.round == ROUND_FLOP
            && ctx.is_overbet_bluffer
            && ctx.to_call > 0
            && !ctx.is_three_flush
            && !ctx.is_four_board_danger
            && is_flop_overbet_or_large
            && self.is_guard_enabled("OVERBET_BLUFF_CATCH")
            && let (Some(f_i), Some(c_i)) = (
                action_idx(ctx.legal_actions, FOLD),
                action_idx(ctx.legal_actions, CALL_CHECK),
            )
            && probs[f_i] > 0.30
        {
            reasons.push("OVERBET_BLUFF_CATCH");
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
            && self.is_guard_enabled("POT_COMMIT_TERMINAL")
            && let Some(f_i) = action_idx(ctx.legal_actions, FOLD)
        {
            let f_prob = probs[f_i];
            if f_prob > 0.0 {
                reasons.push("POT_COMMIT_TERMINAL");
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
