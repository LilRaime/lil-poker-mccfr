use std::collections::HashMap;

use super::ReachContext;
use crate::cfr::abstraction::preflop_strength_tier;
use crate::game::config::{CALL_CHECK, RAISE_MIN};

fn fallback_preflop_weight(pf_idx: usize, ctx: &ReachContext) -> f64 {
    if ctx.opp_3bet_pf {
        /* Condensed 3-bet/4-bet range: AA-TT (0..=4), AKs-AQs (13..=14), AKo-AQo (91, 92) */
        let is_tier1 = pf_idx <= 4 || pf_idx == 13 || pf_idx == 14 || pf_idx == 91 || pf_idx == 92;
        let is_tier2 = (5..=7).contains(&pf_idx)
            || (15..=17).contains(&pf_idx)
            || pf_idx == 21
            || pf_idx == 22
            || pf_idx == 25
            || pf_idx == 93
            || pf_idx == 103;
        if is_tier1 {
            1.00
        } else if is_tier2 {
            0.65
        } else if pf_idx <= 25 || (91..=95).contains(&pf_idx) {
            0.20
        } else {
            0.01 /* Pure junk almost never 3-bets */
        }
    } else if ctx.opp_limped_pf {
        /* Capped limping range: monsters rarely limp, mid pairs/connectors limp often */
        if pf_idx <= 2 || pf_idx == 13 || pf_idx == 91 {
            0.08 /* AA, KK, QQ, AKs, AKo trap rarely */
        } else if pf_idx <= 4 || pf_idx == 14 || pf_idx == 92 {
            0.25 /* JJ, TT, AQs, AQo rarely limp */
        } else if (5..=12).contains(&pf_idx) || (15..=85).contains(&pf_idx) {
            1.00 /* 99-22, suited connectors / suited broadways */
        } else if pf_idx <= 130 {
            0.75 /* Offsuit connectors / broadways */
        } else {
            0.35 /* Lowest junk */
        }
    } else if ctx.opp_raised_pf {
        match preflop_strength_tier(pf_idx) {
            1 | 2 => 1.00,
            3 | 4 => 0.80,
            5 | 6 => 0.30,
            _ => 0.04,
        }
    } else if ctx.opp_called_pf {
        match preflop_strength_tier(pf_idx) {
            1 => 0.35, /* Premium hands rarely just call preflop */
            2 => 0.60,
            3 | 4 => 0.95,
            5 | 6 => 0.65,
            _ => 0.18,
        }
    } else {
        match preflop_strength_tier(pf_idx) {
            1 => 0.50,
            2 => 0.70,
            3 | 4 => 0.85,
            5 | 6 => 0.60,
            _ => 0.35,
        }
    }
}

/** 1. Preflop Bayesian Prior (informed by blueprint strategy when provided) */
pub(super) fn preflop_prior_weight<V: AsRef<[f64]>>(
    pf_idx: usize,
    pf_name: &str,
    strategy_map: Option<&HashMap<String, V>>,
    target_player: Option<usize>,
    ctx: &ReachContext,
) -> f64 {
    let Some(strat_map) = strategy_map else {
        return fallback_preflop_weight(pf_idx, ctx);
    };
    let pf_key_facing_raise = format!("P:{}/r", pf_name);
    let pf_key_root = format!("P:{}/", pf_name);
    let probs_opt = if target_player == Some(1) && (ctx.opp_3bet_pf || ctx.opp_called_pf) {
        strat_map
            .get(&pf_key_facing_raise)
            .or_else(|| strat_map.get(&pf_key_root))
    } else {
        strat_map.get(&pf_key_root)
    };
    let Some(probs_raw) = probs_opt else {
        return fallback_preflop_weight(pf_idx, ctx);
    };
    let probs = probs_raw.as_ref();
    if ctx.opp_3bet_pf {
        let r_prob: f64 = probs.iter().skip(RAISE_MIN as usize).sum();
        r_prob.powi(2).max(0.01)
    } else if ctx.opp_raised_pf {
        let r_prob: f64 = probs.iter().skip(RAISE_MIN as usize).sum();
        r_prob.max(0.03)
    } else if ctx.opp_limped_pf {
        let c_prob = probs.get(CALL_CHECK as usize).copied().unwrap_or(0.5);
        if pf_idx <= 2 || pf_idx == 13 || pf_idx == 91 {
            (c_prob * 0.15).max(0.05)
        } else {
            c_prob.max(0.20)
        }
    } else {
        probs
            .get(CALL_CHECK as usize)
            .copied()
            .unwrap_or(0.5)
            .max(0.05)
    }
}

/** 5. Observed VPIP / PFR Range Width Adaptation */
pub(super) fn style_width_multiplier(
    tier: u8,
    opp_vpip: Option<f64>,
    opp_pfr: Option<f64>,
    opp_raised_pf: bool,
) -> f64 {
    let vpip_mult = if let Some(vpip) = opp_vpip {
        if vpip > 0.60 {
            /* Loose opponent: significantly higher weight on speculative / trash tiers 5..=8 */
            if tier >= 6 {
                1.85
            } else if tier == 5 {
                1.40
            } else {
                1.0
            }
        } else if vpip < 0.26 {
            /* Tight/Nit opponent: heavily discount tiers 5..=8 */
            if tier >= 6 {
                0.18
            } else if tier == 5 {
                0.40
            } else if tier == 4 {
                0.75
            } else {
                1.15
            }
        } else {
            1.0
        }
    } else {
        1.0
    };

    let pfr_mult = if let Some(pfr) = opp_pfr {
        if opp_raised_pf && pfr > 0.42 && tier >= 4 {
            1.45
        } else if opp_raised_pf && pfr < 0.15 && tier >= 4 {
            0.35
        } else {
            1.0
        }
    } else {
        1.0
    };

    vpip_mult * pfr_mult
}
