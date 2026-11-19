/* Multi-Way Table Adaptation (3 to 6 Active Players: Range Tightening & Bluff Damping) */
use super::guards::{action_idx, force_action, zero_and_sum, GuardContext};
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::preflop_strength_tier;
use crate::game::config::*;

/* 6-Max Positional Preflop Open-Raise Chart:
 * Maps seat_position (0=UTG, 1=MP, 2=CO, 3=BTN, 4=SB, 5=BB) to maximum playable open tier. */
pub fn max_open_tier_6max(seat_pos: u8) -> u8 {
    match seat_pos {
        0 => 3, /* UTG: ~15% VPIP (premiums, medium/high pairs, strong suited broadways) */
        1 => 4, /* MP:  ~20% VPIP */
        2 => 5, /* CO:  ~28% VPIP */
        3 => 6, /* BTN: ~42% VPIP */
        4 => 5, /* SB:  ~32% VPIP */
        _ => 5, /* BB:  defend/iso */
    }
}

impl OpponentTracker {
    pub(super) fn apply_multiway_adaptation(&self, probs: &mut [f64], ctx: &GuardContext<'_>) {
        if ctx.active_opponents < 2 {
            return;
        }

        let extra_opps = (ctx.active_opponents - 1).min(5) as i32;
        let fold_idx = action_idx(ctx.legal_actions, FOLD);
        let call_idx = action_idx(ctx.legal_actions, CALL_CHECK);

        if ctx.is_preflop {
            self.apply_multiway_preflop(probs, ctx, extra_opps, fold_idx, call_idx);
        } else {
            self.apply_multiway_postflop(probs, ctx, extra_opps, fold_idx, call_idx);
        }
    }

    fn apply_multiway_preflop(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        extra_opps: i32,
        fold_idx: Option<usize>,
        call_idx: Option<usize>,
    ) {
        let pf_tier = preflop_strength_tier(ctx.bucket);
        let is_offsuit = ctx.bucket >= 91;
        /* Infer approximate 6-max seat tightness from active_opponents count:
         * 5 active opps ~ UTG (pos 0), 4 ~ MP (1), 3 ~ CO (2), 2 ~ BTN/SB (3/4). */
        let approx_seat_pos: u8 = match ctx.active_opponents {
            5.. => 0,
            4 => 1,
            3 => 2,
            _ => 3,
        };
        let pos_max_tier = max_open_tier_6max(approx_seat_pos);

        if pf_tier > pos_max_tier && ctx.to_call <= ctx.bb && ctx.pot_odds > 0.20 {
            /* Tight positional preflop discipline in early/middle 6-max seats: fold below chart */
            force_action(probs, ctx.legal_actions, FOLD);
        } else if pf_tier >= 6 || (pf_tier == 5 && is_offsuit && ctx.active_opponents >= 2) {
            /* Pure trash, steal-tier, or weak offsuit in multi-way pot:
             * never raise and never open-limp; fold to any bet unless priced in from BB (pot_odds <= 0.22)! */
            let removed_raise = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if ctx.to_call > 0 {
                if ctx.pot_odds <= 0.22 && pf_tier <= 7 {
                    if let Some(c_i) = call_idx {
                        probs[c_i] += removed_raise;
                    }
                } else if let Some(f_i) = fold_idx {
                    let call_w = call_idx.map(|ci| probs[ci]).unwrap_or(0.0);
                    if let Some(ci) = call_idx {
                        probs[ci] = 0.0;
                    }
                    probs[f_i] = (probs[f_i] + removed_raise + call_w).max(0.95);
                }
            } else if let Some(c_i) = call_idx {
                probs[c_i] = (probs[c_i] + removed_raise).max(0.95);
            }
        } else if pf_tier == 5 || (pf_tier == 4 && is_offsuit && ctx.active_opponents >= 3) {
            Self::apply_multiway_preflop_speculative(
                probs, ctx, extra_opps, is_offsuit, fold_idx, call_idx,
            );
        } else if (pf_tier == 3 || pf_tier == 4) && ctx.to_call >= ctx.bb * 4 {
            /* Tier 3-4 facing heavy multi-way action: suppress 4-bet shoves, fold offsuit hands */
            let shifted_raise = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_HALF_POT);
            if shifted_raise > 0.0 {
                if let Some(c_i) = call_idx {
                    probs[c_i] += shifted_raise;
                }
            }
            if is_offsuit && ctx.active_opponents >= 3 && ctx.pot_odds > 0.26 {
                if let Some(f_i) = fold_idx {
                    if let Some(c_i) = call_idx {
                        probs[f_i] += probs[c_i] * 0.70;
                        probs[c_i] *= 0.30;
                    }
                }
            }
        }
    }

    fn apply_multiway_preflop_speculative(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        extra_opps: i32,
        is_offsuit: bool,
        fold_idx: Option<usize>,
        call_idx: Option<usize>,
    ) {
        /* Speculative Tier 5 (or offsuit Tier 4 in 4+ way) in multi-way: do not 3-bet bluff;
         * shift removed raise weight into FOLD for offsuit hands so we don't open-limp offsuit hands! */
        let raise_damp = 0.40_f64.powi(extra_opps);
        let mut shifted = 0.0f64;
        for (i, &a) in ctx.legal_actions.iter().enumerate() {
            if a >= RAISE_MIN {
                if ctx.to_call > ctx.bb {
                    shifted += probs[i];
                    probs[i] = 0.0;
                } else {
                    let d = probs[i] * (1.0 - raise_damp);
                    probs[i] -= d;
                    shifted += d;
                }
            }
        }
        if ctx.to_call > ctx.bb * 3 && ctx.pot_odds > 0.25 {
            /* Facing a large preflop raise multi-way with Tier 5 -> fold */
            if let Some(f_i) = fold_idx {
                if let Some(c_i) = call_idx {
                    shifted += probs[c_i] * 0.80;
                    probs[c_i] *= 0.20;
                }
                probs[f_i] = (probs[f_i] + shifted).max(0.85);
            }
        } else if is_offsuit && ctx.to_call >= ctx.bb && ctx.pot_odds > 0.25 {
            /* Never open-limp offsuit speculative hands in multi-way: shift damped raises into fold */
            if let Some(f_i) = fold_idx {
                probs[f_i] += shifted;
            }
        } else if let Some(c_i) = call_idx {
            probs[c_i] += shifted;
        }
    }

    fn apply_multiway_postflop(
        &self,
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        extra_opps: i32,
        fold_idx: Option<usize>,
        call_idx: Option<usize>,
    ) {
        if ctx.bucket < 15 || ctx.bucket == 28 || ctx.bucket == 29 {
            Self::apply_multiway_postflop_air(probs, ctx, extra_opps, fold_idx, call_idx);
        } else if (15..=27).contains(&ctx.bucket) {
            Self::apply_multiway_postflop_draws(probs, ctx, extra_opps, fold_idx, call_idx);
        } else if (30..55).contains(&ctx.bucket) {
            Self::apply_multiway_postflop_one_pair(probs, ctx, fold_idx, call_idx);
        }

        Self::apply_multiway_four_flush_and_shove(probs, ctx, call_idx);
    }

    fn apply_multiway_postflop_air(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        extra_opps: i32,
        fold_idx: Option<usize>,
        call_idx: Option<usize>,
    ) {
        /* 1) Pure air or board-pair air (bucket < 15 || bucket == 28 || bucket == 29):
         *    Fold equity across N opponents drops as p^N -> suppress multi-way bluffs! */
        let bluff_damp = 0.20_f64.powi(extra_opps);
        let mut shifted = 0.0f64;
        for (i, &a) in ctx.legal_actions.iter().enumerate() {
            if a >= RAISE_HALF_POT || (a >= RAISE_MIN && ctx.to_call > 0) {
                shifted += probs[i];
                probs[i] = 0.0;
            } else if a >= RAISE_MIN {
                let d = probs[i] * (1.0 - bluff_damp);
                probs[i] -= d;
                shifted += d;
            }
        }
        if ctx.to_call > 0 {
            if let Some(f_i) = fold_idx {
                if let Some(c_i) = call_idx {
                    shifted += probs[c_i];
                    probs[c_i] = 0.0;
                }
                probs[f_i] = (probs[f_i] + shifted).max(0.95);
            }
        } else if let Some(c_i) = call_idx {
            probs[c_i] += shifted;
        }
    }

    fn apply_multiway_postflop_draws(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        extra_opps: i32,
        fold_idx: Option<usize>,
        call_idx: Option<usize>,
    ) {
        /* 2) Draws (15..=27): Great multi-way implied odds when calling cheap bets,
         *    poor fold equity when semi-bluff raising into multiple players. */
        let semibluff_damp = 0.35_f64.powi(extra_opps);
        let mut shifted = 0.0f64;
        for (i, &a) in ctx.legal_actions.iter().enumerate() {
            if a == ALL_IN || (a >= RAISE_MIN && ctx.to_call > 0) {
                shifted += probs[i];
                probs[i] = 0.0;
            } else if a >= RAISE_MIN {
                let d = probs[i] * (1.0 - semibluff_damp);
                probs[i] -= d;
                shifted += d;
            }
        }
        if ctx.to_call > 0 && ctx.pot_odds > 0.32 && ctx.bucket < 25 {
            /* Non-nut draw facing expensive multi-way bet -> fold */
            if let Some(f_i) = fold_idx {
                probs[f_i] += shifted;
            }
        } else if let Some(c_i) = call_idx {
            probs[c_i] += shifted;
        }
    }

    fn apply_multiway_postflop_one_pair(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        fold_idx: Option<usize>,
        call_idx: Option<usize>,
    ) {
        /* 3) One-Pair & Low/Mid Overpair hands (30..55) in multi-way pots:
         *    Pot control! Avoid bloating pots or stacking off with a single pair against multiple opponents. */
        let mut shifted = 0.0f64;
        for (i, &a) in ctx.legal_actions.iter().enumerate() {
            if a == ALL_IN || (a == RAISE_HALF_POT && ctx.bucket < 50) {
                shifted += probs[i];
                probs[i] = 0.0;
            } else if a >= RAISE_MIN && (ctx.to_call > 0 || ctx.bucket < 46) {
                let d = probs[i] * 0.75;
                probs[i] -= d;
                shifted += d;
            }
        }
        if ctx.to_call > 0
            && ((ctx.bucket < 44 && ctx.pot_odds >= 0.18)
                || (ctx.bucket < 50 && ctx.pot_odds >= 0.28 && ctx.active_opponents >= 3))
        {
            if let Some(f_i) = fold_idx {
                if let Some(c_i) = call_idx {
                    let cd = probs[c_i] * 0.65;
                    probs[c_i] -= cd;
                    shifted += cd;
                }
                probs[f_i] += shifted;
            }
        } else if let Some(c_i) = call_idx {
            probs[c_i] += shifted;
        }
    }

    fn apply_multiway_four_flush_and_shove(
        probs: &mut [f64],
        ctx: &GuardContext<'_>,
        call_idx: Option<usize>,
    ) {
        /* 4) Multi-way 4-Flush Board & Cold-Shove Defense (all buckets):
         *    - On a 4-flush board against multiple opponents, anything weaker than 2nd-Nut Flush (bucket < 82)
         *      must NEVER bet/raise, and must fold to any meaningful bet (>= 4 BB or pot_odds >= 0.18).
         *    - Facing a massive multi-way shove (to_call >= 15 BB and pot_odds > 0.18), require at least
         *      Trips/Set (bucket >= 65) on clean boards or Strong Flush/Full House (bucket >= 80) on 3-flush boards! */
        if ctx.is_four_flush_board && ctx.bucket < 82 {
            let shifted = zero_and_sum(probs, ctx.legal_actions, |a| a >= RAISE_MIN);
            if ctx.to_call > 0 && (ctx.to_call >= ctx.bb * 4 || ctx.pot_odds >= 0.18) {
                force_action(probs, ctx.legal_actions, FOLD);
            } else if let Some(c_i) = call_idx {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        } else if ctx.to_call >= ctx.bb * 15 && ctx.pot_odds > 0.18 {
            let fold_multiway_shove = if ctx.round == 4 {
                ctx.bucket < 65 || (ctx.is_three_flush && ctx.bucket < 80)
            } else {
                ctx.bucket < 57
                    && !(ctx.bucket == 22
                        || ctx.bucket == 23
                        || ctx.bucket == 25
                        || ctx.bucket == 27)
            };
            if fold_multiway_shove {
                force_action(probs, ctx.legal_actions, FOLD);
            }
        }
    }
}
