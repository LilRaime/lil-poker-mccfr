/*! Real-time Decision Engine for Live Poker Client */
use std::collections::HashMap;

use super::protocol::{map_action_index, ActionMapRequest};
use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket, preflop_strength_tier};
use crate::cfr::fallback::{get_holdem_fallback_strategy, FallbackRequest, StrategyLookupIndex};
use crate::cfr::opponent_model::{
    DecisionTrace, OpponentTracker, PolicyRequest, ShortStackRequest,
};
use crate::cfr::subgame::{SubgameContext, SubgameRequest, SubgameSolver};
use crate::game::config::*;
use crate::game::holdem::{Card, Rank, RoundHistory, Suit};

#[derive(Clone, Copy)]
pub struct LiveDecisionRequest<'a> {
    pub hole: [Card; 2],
    pub board: &'a [Card],
    pub legal: &'a [String],
    pub to_call: i32,
    pub pot: i32,
    pub subgame_search: bool,
    pub history: [RoundHistory; 4],
    pub my_player: usize,
    pub contributions: [i32; 2],
    pub raises_this_round: u8,
    pub active_opponents: usize,
    pub big_blind: i32,
    pub my_chips: i32,
    pub current_bet: i32,
    pub strategy_index: Option<&'a StrategyLookupIndex>,
    pub solver: Option<&'a SubgameSolver>,
}

impl Default for LiveDecisionRequest<'_> {
    fn default() -> Self {
        Self {
            hole: [
                Card::new(Rank::Ace, Suit::Spades),
                Card::new(Rank::King, Suit::Spades),
            ],
            board: &[],
            legal: &[],
            to_call: 0,
            pot: 0,
            subgame_search: false,
            history: [RoundHistory::new(); 4],
            my_player: 0,
            contributions: [SMALL_BLIND, BIG_BLIND],
            raises_this_round: 0,
            active_opponents: 1,
            big_blind: BIG_BLIND,
            my_chips: 0,
            current_bet: 0,
            strategy_index: None,
            solver: None,
        }
    }
}

pub fn decide_action(
    req: &LiveDecisionRequest<'_>,
    strategy_map: &HashMap<String, Vec<f64>>,
    tracker: &OpponentTracker,
    solver_ctx: Option<&mut SubgameContext>,
) -> (String, i32) {
    let (act, amt, _) = decide_action_traced(req, strategy_map, tracker, solver_ctx);
    (act, amt)
}

pub fn decide_action_traced(
    req: &LiveDecisionRequest<'_>,
    strategy_map: &HashMap<String, Vec<f64>>,
    tracker: &OpponentTracker,
    solver_ctx: Option<&mut SubgameContext>,
) -> (String, i32, DecisionTrace) {
    let bb = if req.big_blind > 0 {
        req.big_blind
    } else {
        BIG_BLIND
    };
    let active_opponents = req.active_opponents.max(1);
    let (to_call, pot) = normalize_uncallable_bet(req.to_call, req.pot, req.my_chips);
    let round = match req.board.len() {
        0 => 1,
        3 => 2,
        4 => 3,
        _ => 4,
    };
    let legal_u8 = derive_legal_u8(req.legal, to_call);
    let current_bucket = if round == 1 {
        preflop_bucket(req.hole[0], req.hole[1]).0
    } else {
        postflop_equity_bucket(&req.hole, req.board)
    };
    let state = LiveDecisionState {
        hole: &req.hole,
        board: req.board,
        legal: req.legal,
        legal_u8,
        round,
        bucket: current_bucket,
        to_call,
        pot,
        bb,
        my_chips: req.my_chips,
        current_bet: req.current_bet,
        active_opponents,
        my_player: req.my_player,
        history: &req.history,
    };

    if let Some(early) = try_extreme_pot_commit_rescue(&state) {
        return early;
    }
    if let Some(short_res) = try_short_stack_nash_decision(&state, tracker) {
        return short_res;
    }

    let (raw_probs, use_subgame) = resolve_raw_strategy_probs(
        &state,
        tracker,
        LiveSubgameParams {
            strategy_map,
            subgame_search: req.subgame_search,
            contributions: req.contributions,
            raises_this_round: req.raises_this_round,
            strategy_index: req.strategy_index,
            solver: req.solver,
            solver_ctx,
        },
    );

    let mut rng = rand::thread_rng();
    let (chosen_u8, mut trace) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &raw_probs,
            legal_actions: &state.legal_u8,
            bucket: state.bucket,
            round: state.round,
            to_call: state.to_call,
            pot: state.pot,
            hole: Some(state.hole),
            board: Some(state.board),
            active_opponents: state.active_opponents,
            table_bb: state.bb,
            my_chips: state.my_chips,
            from_subgame: use_subgame,
        },
        &mut rng,
    );
    let mut chosen_idx = chosen_u8 as usize;
    apply_live_post_selection_guards(&state, tracker, &mut chosen_idx, &mut trace);
    trace.final_action = chosen_idx as u8;

    let is_wet = has_wet_suits(state.board);
    if let Some((act, amt)) = map_action_index(&ActionMapRequest {
        action_idx: chosen_idx,
        legal: state.legal,
        to_call: state.to_call,
        pot: state.pot,
        is_wet,
        round: state.round,
        table_bb: state.bb,
        my_chips: state.my_chips,
        current_bet: state.current_bet,
    }) {
        return (act, amt, trace);
    }

    let (def_act, def_amt) = safe_default_live_action(&state);
    (def_act, def_amt, trace)
}

struct LiveDecisionState<'a> {
    hole: &'a [Card; 2],
    board: &'a [Card],
    legal: &'a [String],
    legal_u8: Vec<u8>,
    round: u8,
    bucket: usize,
    to_call: i32,
    pot: i32,
    bb: i32,
    my_chips: i32,
    current_bet: i32,
    active_opponents: usize,
    my_player: usize,
    history: &'a [RoundHistory; 4],
}

impl LiveDecisionState<'_> {
    fn has(&self, action: &str) -> bool {
        self.legal.iter().any(|l| l.eq_ignore_ascii_case(action))
    }
}

struct LiveSubgameParams<'a> {
    strategy_map: &'a HashMap<String, Vec<f64>>,
    subgame_search: bool,
    contributions: [i32; 2],
    raises_this_round: u8,
    strategy_index: Option<&'a StrategyLookupIndex>,
    solver: Option<&'a SubgameSolver>,
    solver_ctx: Option<&'a mut SubgameContext>,
}

fn normalize_uncallable_bet(to_call: i32, pot: i32, my_chips: i32) -> (i32, i32) {
    if my_chips > 0 && to_call > my_chips {
        let uncallable = to_call - my_chips;
        let eff_pot = if pot > uncallable + my_chips {
            pot - uncallable
        } else {
            pot.max(my_chips)
        };
        (my_chips, eff_pot)
    } else {
        (to_call, pot)
    }
}

fn derive_legal_u8(legal: &[String], to_call: i32) -> Vec<u8> {
    let has = |a: &str| legal.iter().any(|l| l.eq_ignore_ascii_case(a));
    let mut legal_u8: Vec<u8> = Vec::new();
    if to_call > 0 && has("fold") {
        legal_u8.push(FOLD);
    }
    /* Action Bridge: if facing a bet that puts us all-in and the server only lists ["fold", "all_in"],
     * include CALL_CHECK so CFR/fallback call probabilities are preserved and mapped to all_in! */
    if has("check") || has("call") || (to_call > 0 && (has("all_in") || has("allin"))) {
        legal_u8.push(CALL_CHECK);
    }
    if has("raise") || has("bet") {
        legal_u8.push(RAISE_MIN);
        legal_u8.push(RAISE_THIRD_POT);
        legal_u8.push(RAISE_HALF_POT);
        legal_u8.push(ALL_IN);
    }
    if (has("allin") || has("all_in")) && !legal_u8.contains(&ALL_IN) {
        legal_u8.push(ALL_IN);
    }
    if legal_u8.is_empty() {
        if has("check") || has("call") {
            legal_u8.push(CALL_CHECK);
        } else if has("fold") {
            legal_u8.push(FOLD);
        } else if has("allin") || has("all_in") {
            legal_u8.push(ALL_IN);
        }
    }
    legal_u8
}

fn try_extreme_pot_commit_rescue(
    s: &LiveDecisionState<'_>,
) -> Option<(String, i32, DecisionTrace)> {
    /* Unconditional Micro-Stack / Extreme Pot-Odds Commitment Rescue:
     * Never fold preflop or postflop when priced in at <= 12.5% pot odds or facing <= 0.5 BB with <= 0.5 BB left! */
    if s.to_call <= 0 || !(s.has("call") || s.has("all_in") || s.has("allin")) {
        return None;
    }
    let new_pot = (s.pot + s.to_call).max(1);
    let is_extreme_commit =
        s.to_call * 8 <= new_pot || (s.my_chips > 0 && s.my_chips <= s.bb / 2 && s.pot >= s.bb * 4);
    if !(is_extreme_commit
        && (s.round >= 2 || preflop_strength_tier(s.bucket) <= 6 || s.my_chips <= s.to_call))
    {
        return None;
    }
    let pot_odds_bps =
        ((s.to_call as f64 / new_pot as f64).clamp(0.0, 1.0) * 1000.0).round() as u16;
    let tr = DecisionTrace {
        bucket: s.bucket,
        round: s.round,
        pot_odds_bps,
        raw_top_action: CALL_CHECK,
        final_action: CALL_CHECK,
        guard_reasons: vec!["POT_COMMIT_DECISION_RESCUE"],
    };
    let tok = if s.has("call") {
        "call"
    } else if s.has("all_in") {
        "all_in"
    } else {
        "allin"
    };
    Some((tok.to_string(), 0, tr))
}

fn try_short_stack_nash_decision(
    s: &LiveDecisionState<'_>,
    tracker: &OpponentTracker,
) -> Option<(String, i32, DecisionTrace)> {
    /* Short-Stack Nash Push/Fold Engine: <= 12 BB on preflop */
    if s.round != 1 || s.my_chips <= 0 || s.my_chips > s.bb * 12 {
        return None;
    }
    let (short_act, reason) = tracker.select_short_stack_preflop_action(&ShortStackRequest {
        bucket: s.bucket,
        to_call: s.to_call,
        pot: s.pot,
        active_opponents: s.active_opponents,
        table_bb: s.bb,
        my_chips: s.my_chips,
        legal_actions: &s.legal_u8,
    })?;
    let total_pot = (s.pot + s.to_call).max(1);
    let pot_odds = s.to_call as f64 / total_pot as f64;
    let pot_odds_bps = (pot_odds.clamp(0.0, 1.0) * 1000.0).round() as u16;
    let tr = DecisionTrace {
        bucket: s.bucket,
        round: 1,
        pot_odds_bps,
        raw_top_action: short_act,
        final_action: short_act,
        guard_reasons: vec![reason],
    };
    match short_act {
        ALL_IN => {
            let my_posted_bet = s.current_bet.saturating_sub(s.to_call);
            let allin_total = s.my_chips + my_posted_bet;
            if s.to_call >= s.my_chips && s.has("call") {
                Some(("call".to_string(), 0, tr))
            } else if s.has("all_in") {
                Some(("all_in".to_string(), 0, tr))
            } else if s.has("allin") {
                Some(("allin".to_string(), 0, tr))
            } else if s.has("raise") {
                Some(("raise".to_string(), allin_total, tr))
            } else if s.has("bet") {
                Some(("bet".to_string(), allin_total, tr))
            } else if s.has("call") {
                Some(("call".to_string(), 0, tr))
            } else {
                Some(("check".to_string(), 0, tr))
            }
        }
        CALL_CHECK => {
            if s.to_call == 0 && s.has("check") {
                Some(("check".to_string(), 0, tr))
            } else if s.has("call") {
                Some(("call".to_string(), 0, tr))
            } else if s.has("all_in") {
                Some(("all_in".to_string(), 0, tr))
            } else if s.has("allin") {
                Some(("allin".to_string(), 0, tr))
            } else if s.has("check") {
                Some(("check".to_string(), 0, tr))
            } else {
                None
            }
        }
        _ if s.has("fold") => Some(("fold".to_string(), 0, tr)),
        _ => None,
    }
}

fn resolve_raw_strategy_probs(
    s: &LiveDecisionState<'_>,
    tracker: &OpponentTracker,
    sp: LiveSubgameParams<'_>,
) -> (Vec<f64>, bool) {
    /* 1. Real-time Subgame Search (Turn & River, or Flop for medium/big pots when Heads-Up 1v1) */
    let flop_trigger_pot = (s.bb * 8).max(1);
    let use_subgame = sp.subgame_search
        && s.active_opponents == 1
        && (s.round >= 3 || (s.round == 2 && s.pot >= flop_trigger_pot));

    let (opp_vpip_opt, opp_pfr_opt) = if tracker.is_overbet_bluffer() {
        (
            Some(tracker.vpip_ratio()),
            Some(tracker.pfr_ratio().max(0.50)),
        )
    } else if tracker.total_hands >= 6 {
        (Some(tracker.vpip_ratio()), Some(tracker.pfr_ratio()))
    } else {
        (None, None)
    };

    let raw_probs = if use_subgame {
        let default_solver = SubgameSolver::default();
        let active_solver = sp.solver.unwrap_or(&default_solver);
        active_solver.solve_with_strategy(
            &SubgameRequest {
                hole: *s.hole,
                board: s.board,
                round: s.round,
                my_player: s.my_player,
                contributions: sp.contributions,
                raises_this_round: sp.raises_this_round,
                opp_vpip: opp_vpip_opt,
                opp_pfr: opp_pfr_opt,
            },
            s.history,
            Some(sp.strategy_map),
            sp.solver_ctx,
        )
    } else {
        /* 2. Abstract Strategy Model Lookup (single-model multi-stack aware) with O(1) Fallback */
        let eff_stack_bb = if s.my_chips > 0 && s.bb > 0 {
            s.my_chips / s.bb
        } else {
            100
        };
        if let Some(p) = crate::cfr::fallback::lookup_strategy_hierarchical_owned_with_stack(
            |k| sp.strategy_map.get(k).cloned(),
            s.hole,
            s.board,
            s.round,
            s.history,
            eff_stack_bb,
        ) {
            p
        } else {
            let req = FallbackRequest {
                hole: *s.hole,
                board: s.board,
                round: s.round,
                to_call: s.to_call,
                pot: s.pot,
                legal: &s.legal_u8,
                table_bb: s.bb,
            };
            if let Some(idx) = sp.strategy_index {
                idx.get_blended_fallback(&req)
            } else {
                get_holdem_fallback_strategy(&req).to_vec()
            }
        }
    };
    (raw_probs, use_subgame)
}

fn apply_live_post_selection_guards(
    s: &LiveDecisionState<'_>,
    tracker: &OpponentTracker,
    chosen_idx: &mut usize,
    trace: &mut DecisionTrace,
) {
    apply_live_street_action_guards(s, tracker, chosen_idx, trace);
    apply_live_commit_and_spr_guards(s, chosen_idx, trace);
}

fn apply_live_street_action_guards(
    s: &LiveDecisionState<'_>,
    tracker: &OpponentTracker,
    chosen_idx: &mut usize,
    trace: &mut DecisionTrace,
) {
    if s.round == 4 && s.to_call == 0 && s.bucket < 65 && *chosen_idx >= RAISE_MIN as usize {
        let turn_hist = s.history[2].as_slice();
        let turn_raises = turn_hist.iter().filter(|&&a| a >= RAISE_MIN).count();
        /* On Turn, OOP (my_player == 1) acts on even indices (0, 2, ...), IP (my_player == 0) on odd indices (1, 3, ...). */
        let opp_was_last_turn_raiser = turn_hist
            .iter()
            .enumerate()
            .rev()
            .find(|&(_, &a)| a >= RAISE_MIN)
            .map(|(idx, _)| {
                let is_oop_action = idx % 2 == 0;
                (s.my_player == 1 && !is_oop_action) || (s.my_player == 0 && is_oop_action)
            })
            .unwrap_or(false);
        if turn_raises >= 2 || (s.my_player == 1 && opp_was_last_turn_raiser) {
            *chosen_idx = CALL_CHECK as usize;
            trace.guard_reasons.push("RIVER_CHECK_TO_TURN_AGGRESSOR");
        }
    }
    if s.round == 2
        && s.to_call == 0
        && s.my_player == 1
        && *chosen_idx == RAISE_MIN as usize
        && s.bucket < 40
        && tracker.is_guard_enabled("FLOP_OOP_MICRO_DONK_SUPPRESS")
    {
        let pf_hist = s.history[0].as_slice();
        let pf_raises = pf_hist.iter().filter(|&&a| a >= RAISE_MIN).count();
        let opp_was_pf_aggressor =
            pf_raises == 1 && pf_hist.first().copied().unwrap_or(0) >= RAISE_MIN;
        if opp_was_pf_aggressor {
            *chosen_idx = CALL_CHECK as usize;
            trace.guard_reasons.push("FLOP_OOP_MICRO_DONK_SUPPRESS");
        }
    }
}

fn apply_live_commit_and_spr_guards(
    s: &LiveDecisionState<'_>,
    chosen_idx: &mut usize,
    trace: &mut DecisionTrace,
) {
    if s.to_call > 0 && *chosen_idx == FOLD as usize && s.legal_u8.contains(&CALL_CHECK) {
        let new_pot = (s.pot + s.to_call).max(1);
        let is_postflop_micro_commit = s.round >= 2
            && (s.to_call * 8 <= new_pot
                || (s.to_call * 6 <= new_pot && s.bucket >= 15)
                || (s.my_chips > 0 && s.my_chips <= s.bb && s.pot >= s.bb * 4));
        let is_preflop_priced_in = s.round == 1
            && (s.to_call <= s.bb * 2 || preflop_strength_tier(s.bucket) <= 6)
            && s.to_call * 6 <= new_pot;
        if is_postflop_micro_commit || is_preflop_priced_in {
            *chosen_idx = CALL_CHECK as usize;
            trace.guard_reasons.push("POT_COMMIT_DECISION_RESCUE");
        }
    }
    if s.round >= 2 && s.my_chips > 0 {
        let is_four_board_pair = s.round >= 3 && matches!(s.bucket, 28 | 40 | 50 | 57);
        let is_bloated_flop_weak = s.round == 2
            && ((s.bucket < 15 && (s.my_chips <= s.pot * 2 || s.pot >= s.bb * 8))
                || ((28..=37).contains(&s.bucket) && s.pot >= s.bb * 10));
        if s.to_call == 0
            && *chosen_idx >= RAISE_MIN as usize
            && (is_four_board_pair
                || is_bloated_flop_weak
                || (s.bucket < 11
                    && *chosen_idx >= RAISE_THIRD_POT as usize
                    && (s.my_chips <= s.pot * 2 || s.pot >= s.bb * 8)))
        {
            *chosen_idx = CALL_CHECK as usize;
            trace.guard_reasons.push("LOW_SPR_BLUFF_SUPPRESS");
        } else if s.to_call > s.bb
            && (s.bucket < 15 || s.bucket == 28)
            && *chosen_idx == CALL_CHECK as usize
        {
            let remaining = s.my_chips - s.to_call;
            let new_pot = (s.pot + s.to_call).max(1);
            if s.to_call * 6 > new_pot
                && (remaining * 2 < new_pot || s.to_call * 10 >= s.my_chips * 3)
            {
                *chosen_idx = FOLD as usize;
                trace.guard_reasons.push("LOW_SPR_AIR_CALL_FOLD");
            }
        }
    }
}

fn has_wet_suits(board: &[Card]) -> bool {
    board.len() >= 3 && {
        let mut suits = [0u8; 4];
        for c in board {
            suits[c.suit as usize] += 1;
        }
        suits.iter().any(|&cnt| cnt >= 2)
    }
}

fn safe_default_live_action(s: &LiveDecisionState<'_>) -> (String, i32) {
    /* Safe default: Check (if to_call == 0) -> Call -> All-in (for nuts facing all-in) -> Check -> Fold */
    if s.to_call == 0 && s.has("check") {
        ("check".to_string(), 0)
    } else if s.has("call") {
        ("call".to_string(), 0)
    } else if s.to_call > 0 && s.round >= 2 && s.bucket >= 79 && s.has("all_in") {
        ("all_in".to_string(), 0)
    } else if s.to_call > 0 && s.round >= 2 && s.bucket >= 79 && s.has("allin") {
        ("allin".to_string(), 0)
    } else if s.has("check") {
        ("check".to_string(), 0)
    } else {
        ("fold".to_string(), 0)
    }
}
