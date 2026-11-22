/*! Purified Action Sampling and Orchestration Pipeline */
use super::exploit::ExploitRequest;
use super::tracker::OpponentTracker;
use crate::cfr::abstraction::{
    nash_call_shove_max_bb, nash_open_push_max_bb, preflop_strength_tier,
};
use crate::game::config::{
    ALL_IN, BIG_BLIND, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, ROUND_FLOP,
};
use crate::game::holdem::Card;
use rand::{Rng, RngExt};

#[derive(Debug, Clone, Copy)]
pub struct PolicyRequest<'a> {
    pub raw_probs: &'a [f64],
    pub legal_actions: &'a [u8],
    pub bucket: usize,
    pub round: u8,
    pub to_call: i32,
    pub pot: i32,
    pub hole: Option<&'a [Card; 2]>,
    pub board: Option<&'a [Card]>,
    pub active_opponents: usize,
    pub table_bb: i32,
    pub my_chips: i32,
    pub opp_chips: Option<i32>,
    pub from_subgame: bool,
}

impl Default for PolicyRequest<'_> {
    fn default() -> Self {
        Self {
            raw_probs: &[],
            legal_actions: &[],
            bucket: 0,
            round: ROUND_FLOP,
            to_call: 0,
            pot: 0,
            hole: None,
            board: None,
            active_opponents: 1,
            table_bb: BIG_BLIND,
            my_chips: 0,
            opp_chips: None,
            from_subgame: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ShortStackRequest<'a> {
    pub bucket: usize,
    pub to_call: i32,
    pub pot: i32,
    pub active_opponents: usize,
    pub table_bb: i32,
    pub my_chips: i32,
    pub opp_chips: Option<i32>,
    pub legal_actions: &'a [u8],
}

impl Default for ShortStackRequest<'_> {
    fn default() -> Self {
        Self {
            bucket: 0,
            to_call: 0,
            pot: 0,
            active_opponents: 1,
            table_bb: BIG_BLIND,
            my_chips: 0,
            opp_chips: None,
            legal_actions: &[],
        }
    }
}

/** Structured diagnostic trace of a purified action decision */
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecisionTrace {
    pub bucket: usize,
    pub round: u8,
    pub pot_odds_bps: u16,
    pub raw_top_action: u8,
    pub final_action: u8,
    pub guard_reasons: Vec<&'static str>,
}

impl OpponentTracker {
    /** Contextual purified action sampling with opponent exploitation and guard pipeline */
    pub fn select_action<R: Rng>(&self, req: &PolicyRequest<'_>, rng: &mut R) -> u8 {
        self.select_action_traced(req, rng).0
    }

    /** Contextual purified action sampling that also returns a structured `DecisionTrace` */
    pub fn select_action_traced<R: Rng>(
        &self,
        req: &PolicyRequest<'_>,
        rng: &mut R,
    ) -> (u8, DecisionTrace) {
        let bb = if req.table_bb > 0 {
            req.table_bb
        } else {
            BIG_BLIND
        };
        let eff_allin_threshold = (bb * 35).max(1);
        let active_opponents = req.active_opponents.max(1);
        let raw_top_action = top_legal_action(req.raw_probs, req.legal_actions);
        let adjusted_probs = self.adjust_strategy(&ExploitRequest {
            blueprint_probs: req.raw_probs,
            legal_actions: req.legal_actions,
            bucket: req.bucket,
            round: req.round,
            to_call: req.to_call,
            table_bb: bb,
        });

        let mut probs: Vec<f64> = req
            .legal_actions
            .iter()
            .map(|&a| {
                adjusted_probs
                    .get(a as usize)
                    .copied()
                    .unwrap_or(0.0)
                    .max(0.0)
            })
            .collect();

        let total_pot = req.pot + req.to_call;
        let pot_odds = if total_pot > 0 {
            req.to_call as f64 / total_pot as f64
        } else {
            1.0
        };
        let mut reasons: Vec<&'static str> = Vec::with_capacity(4);
        let is_three_flush = has_three_flush(req.board);
        let eff_my_chips = effective_remaining_chips(req.my_chips, req.opp_chips, req.to_call);

        /* Priority Pipeline (Tier 1 Soft Shifts -> Tier 2 Hard Raise Caps -> Multiway -> Tier 3 Terminal Fold / Pot-Commit) */
        self.apply_preflop_and_postflop_guards(
            &mut probs,
            super::guards::GuardInput {
                legal_actions: req.legal_actions,
                bucket: req.bucket,
                round: req.round,
                to_call: req.to_call,
                pot: req.pot,
                hole: req.hole,
                board: req.board,
                active_opponents,
                bb,
                my_chips: eff_my_chips,
                eff_allin_threshold,
                pot_odds,
                is_three_flush,
                from_subgame: req.from_subgame,
            },
            &mut reasons,
        );

        let chosen = purify_and_sample(&mut probs, req.legal_actions, rng);

        let trace = DecisionTrace {
            bucket: req.bucket,
            round: req.round,
            pot_odds_bps: (pot_odds.clamp(0.0, 1.0) * 1000.0).round() as u16,
            raw_top_action,
            final_action: chosen,
            guard_reasons: reasons,
        };
        (chosen, trace)
    }

    /** Unified <= 12 BB Nash Push/Fold preflop engine shared by live play and simulation.
     * Returns `Some((action, reason_code))` when stack <= 12 BB preflop, else `None`. */
    pub fn select_short_stack_preflop_action(
        &self,
        req: &ShortStackRequest<'_>,
    ) -> Option<(u8, &'static str)> {
        let bb = if req.table_bb > 0 {
            req.table_bb
        } else {
            BIG_BLIND
        };
        let eff_chips = effective_remaining_chips(req.my_chips, req.opp_chips, req.to_call);
        if eff_chips <= 0 || eff_chips > bb * 12 {
            return None;
        }
        let active_opponents = req.active_opponents.max(1);
        let pf_tier = preflop_strength_tier(req.bucket);
        let is_maniac = self.total_actions() >= 10
            && (self.raise_ratio() > 0.38
                || (self.pfr_ratio() > 0.50 && self.raise_ratio() > 0.28)
                || (self.vpip_ratio() > 0.82 && self.raise_ratio() > 0.28));
        let shove_action = if req.legal_actions.contains(&ALL_IN) {
            Some(ALL_IN)
        } else if req.legal_actions.contains(&RAISE_HALF_POT) {
            Some(RAISE_HALF_POT)
        } else if req.legal_actions.contains(&RAISE_MIN) {
            Some(RAISE_MIN)
        } else {
            None
        };
        let has_call_check = req.legal_actions.contains(&CALL_CHECK);
        let ctx = ShortStackCtx {
            bucket: req.bucket,
            to_call: req.to_call,
            pot: req.pot,
            active_opponents,
            bb,
            my_chips: eff_chips,
            pf_tier,
            max_open_push_bb: nash_open_push_max_bb(req.bucket, active_opponents),
            max_call_bb: nash_call_shove_max_bb(req.bucket, active_opponents, is_maniac),
            is_maniac,
            pot_odds: req.to_call as f64 / (req.pot + req.to_call).max(1) as f64,
            call_only_allin: req.to_call > 0 && req.to_call >= eff_chips,
            shove_action,
            call_action: if has_call_check { CALL_CHECK } else { ALL_IN },
            can_call: has_call_check || req.legal_actions.contains(&ALL_IN),
            can_fold: req.legal_actions.contains(&FOLD),
            has_call_check,
        };

        if ctx.pf_tier <= 3 {
            decide_short_stack_premium(&ctx)
        } else if ctx.pf_tier <= 5 {
            decide_short_stack_mid_tier(&ctx)
        } else {
            decide_short_stack_low_tier(&ctx)
        }
    }
}

struct ShortStackCtx {
    bucket: usize,
    to_call: i32,
    pot: i32,
    active_opponents: usize,
    bb: i32,
    my_chips: i32,
    pf_tier: u8,
    max_open_push_bb: i32,
    max_call_bb: i32,
    is_maniac: bool,
    pot_odds: f64,
    call_only_allin: bool,
    shove_action: Option<u8>,
    call_action: u8,
    can_call: bool,
    can_fold: bool,
    has_call_check: bool,
}

fn top_legal_action(raw_probs: &[f64], legal_actions: &[u8]) -> u8 {
    legal_actions
        .iter()
        .copied()
        .max_by(|&a, &b| {
            let pa = raw_probs.get(a as usize).copied().unwrap_or(0.0);
            let pb = raw_probs.get(b as usize).copied().unwrap_or(0.0);
            pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(CALL_CHECK)
}

fn has_three_flush(board: Option<&[Card]>) -> bool {
    board.is_some_and(|b| {
        let mut sc = [0u8; 4];
        for c in b {
            sc[c.suit as usize] += 1;
        }
        sc.iter().any(|&cnt| cnt >= 3)
    })
}

fn purify_and_sample<R: Rng>(probs: &mut [f64], legal_actions: &[u8], rng: &mut R) -> u8 {
    /* Pre-clamp normalization & exploration noise clamping (< 2.5% when clear peak > 20%) */
    let pre_sum: f64 = probs.iter().sum();
    if pre_sum > 1e-12 {
        for p in probs.iter_mut() {
            *p /= pre_sum;
        }
    }
    let max_prob = probs.iter().copied().fold(0.0f64, f64::max);
    if max_prob > 0.20 {
        for p in probs.iter_mut() {
            if *p < 0.025 {
                *p = 0.0;
            }
        }
    }

    /* Mild Temperature Sharpening (T = 0.85 => power 1.18) & weighted sampling */
    let temp_inv = 1.18;
    for p in probs.iter_mut() {
        if *p > 0.0 {
            *p = p.powf(temp_inv);
        }
    }

    let sum: f64 = probs.iter().sum();
    if sum > 1e-12 {
        let norm: Vec<f64> = probs.iter().map(|p| p / sum).collect();
        let r: f64 = rng.random();
        let mut cum = 0.0;
        let mut picked = legal_actions.last().copied().unwrap_or(CALL_CHECK);
        for (i, &p) in norm.iter().enumerate() {
            cum += p;
            if r < cum {
                picked = legal_actions[i];
                break;
            }
        }
        picked
    } else if let Some(&c) = legal_actions.iter().find(|&&a| a == CALL_CHECK) {
        c
    } else {
        *legal_actions.first().unwrap_or(&CALL_CHECK)
    }
}

/** Effective remaining chips hero can play for, distinguishing unknown opponent stack (None) from all-in (Some(0)) */
#[inline]
pub fn effective_remaining_chips(my_chips: i32, opp_chips: Option<i32>, to_call: i32) -> i32 {
    match opp_chips {
        Some(opp) if my_chips > 0 => my_chips.min(opp.max(0) + to_call.max(0)),
        _ => my_chips,
    }
}

fn decide_short_stack_premium(c: &ShortStackCtx) -> Option<(u8, &'static str)> {
    if c.call_only_allin && c.can_call {
        Some((c.call_action, "SHORT_STACK_NASH_PUSH"))
    } else if let Some(sa) = c.shove_action {
        Some((sa, "SHORT_STACK_NASH_PUSH"))
    } else if c.can_call {
        Some((c.call_action, "SHORT_STACK_NASH_PUSH"))
    } else {
        None
    }
}

fn decide_short_stack_mid_tier(c: &ShortStackCtx) -> Option<(u8, &'static str)> {
    if c.to_call == 0 {
        if c.bucket <= 26
            && let Some(sa) = c.shove_action
        {
            return Some((sa, "SHORT_STACK_BB_ISO_PUSH"));
        }
        if c.has_call_check {
            return Some((CALL_CHECK, "SHORT_STACK_BB_FREE_CHECK"));
        }
    } else if c.to_call <= c.bb / 2 && c.pot <= c.bb * 2 && c.my_chips <= c.max_open_push_bb * c.bb
    {
        if let Some(sa) = c.shove_action {
            return Some((sa, "SHORT_STACK_NASH_PUSH"));
        }
    } else if c.to_call <= c.bb && c.pot <= c.bb * 4 {
        if (c.bucket <= 26 || (c.pf_tier == 4 && c.my_chips <= c.bb * 7))
            && let Some(sa) = c.shove_action
        {
            return Some((sa, "SHORT_STACK_BB_3BET_SHOVE"));
        }
        if c.can_call {
            return Some((c.call_action, "SHORT_STACK_BB_FLAT_CALL"));
        }
    } else if c.pot_odds <= 0.33
        || c.my_chips <= c.bb * 2
        || (c.to_call <= c.max_call_bb * c.bb && (c.pf_tier <= 5 || c.is_maniac))
    {
        if c.can_call {
            return Some((c.call_action, "SHORT_STACK_PRICED_IN_CALL"));
        }
    } else if c.can_fold {
        return Some((FOLD, "SHORT_STACK_FOLD"));
    }
    None
}

fn decide_short_stack_low_tier(c: &ShortStackCtx) -> Option<(u8, &'static str)> {
    let is_hu_sb_open = c.active_opponents == 1 && c.to_call <= c.bb / 2 && c.pot <= c.bb * 2;
    if c.to_call > 0
        && c.to_call <= c.bb / 2
        && c.pot <= c.bb * 2
        && (c.my_chips <= c.max_open_push_bb * c.bb
            || (c.active_opponents == 1 && c.my_chips <= c.bb * 2)
            || (c.active_opponents == 1 && c.my_chips <= c.bb * 3 && c.bucket != 158))
    {
        if let Some(sa) = c.shove_action {
            return Some((sa, "SHORT_STACK_NASH_PUSH"));
        }
    } else if c.to_call > 0
        && ((!is_hu_sb_open && c.pot_odds <= 0.28 && c.pf_tier <= 6)
            || (c.active_opponents == 1
                && !is_hu_sb_open
                && c.to_call <= c.bb
                && c.pot <= c.bb * 3
                && c.pot_odds <= 0.28
                && (c.pf_tier <= 7 || (c.my_chips <= c.bb * 7 && c.bucket < 135)))
            || (!is_hu_sb_open && c.pot_odds <= 0.22 && (c.pf_tier <= 6 || c.to_call <= c.bb * 2))
            || c.my_chips <= c.bb * 2
            || (c.is_maniac && c.to_call <= c.max_call_bb * c.bb && c.pf_tier <= 6))
    {
        if c.can_call {
            return Some((c.call_action, "SHORT_STACK_PRICED_IN_CALL"));
        }
    } else if c.to_call == 0 && c.has_call_check {
        return Some((CALL_CHECK, "SHORT_STACK_BB_FREE_CHECK"));
    } else if c.can_fold {
        return Some((FOLD, "SHORT_STACK_FOLD"));
    }
    None
}
