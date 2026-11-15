/* Real-time Decision Engine for Live Poker Client */
use std::collections::HashMap;

use super::protocol::map_action_index_full;
use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket, preflop_strength_tier};
use crate::cfr::fallback::{get_holdem_fallback_strategy_bb, StrategyLookupIndex};
use crate::cfr::opponent_model::{DecisionTrace, OpponentTracker};
use crate::cfr::subgame::{SubgameContext, SubgameSolver};
use crate::game::config::*;
use crate::game::holdem::{Card, RoundHistory};

#[allow(clippy::too_many_arguments)]
pub fn decide_action(
    hole: &[Card; 2],
    board: &[Card],
    legal: &[String],
    to_call: i32,
    pot: i32,
    strategy_map: &HashMap<String, Vec<f64>>,
    subgame_search: bool,
    tracker: &OpponentTracker,
    history: &[RoundHistory; 4],
    my_player: usize,
    contributions: [i32; 2],
    raises_this_round: u8,
    strategy_index: Option<&StrategyLookupIndex>,
    solver: Option<&SubgameSolver>,
    solver_ctx: Option<&mut SubgameContext>,
) -> (String, i32) {
    decide_action_multiway(
        hole,
        board,
        legal,
        to_call,
        pot,
        strategy_map,
        subgame_search,
        tracker,
        history,
        my_player,
        contributions,
        raises_this_round,
        strategy_index,
        solver,
        solver_ctx,
        1,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn decide_action_multiway(
    hole: &[Card; 2],
    board: &[Card],
    legal: &[String],
    to_call: i32,
    pot: i32,
    strategy_map: &HashMap<String, Vec<f64>>,
    subgame_search: bool,
    tracker: &OpponentTracker,
    history: &[RoundHistory; 4],
    my_player: usize,
    contributions: [i32; 2],
    raises_this_round: u8,
    strategy_index: Option<&StrategyLookupIndex>,
    solver: Option<&SubgameSolver>,
    solver_ctx: Option<&mut SubgameContext>,
    active_opponents: usize,
) -> (String, i32) {
    decide_action_multiway_context(
        hole,
        board,
        legal,
        to_call,
        pot,
        strategy_map,
        subgame_search,
        tracker,
        history,
        my_player,
        contributions,
        raises_this_round,
        strategy_index,
        solver,
        solver_ctx,
        active_opponents,
        BIG_BLIND,
        0,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn decide_action_multiway_context(
    hole: &[Card; 2],
    board: &[Card],
    legal: &[String],
    to_call: i32,
    pot: i32,
    strategy_map: &HashMap<String, Vec<f64>>,
    subgame_search: bool,
    tracker: &OpponentTracker,
    history: &[RoundHistory; 4],
    my_player: usize,
    contributions: [i32; 2],
    raises_this_round: u8,
    strategy_index: Option<&StrategyLookupIndex>,
    solver: Option<&SubgameSolver>,
    solver_ctx: Option<&mut SubgameContext>,
    active_opponents: usize,
    big_blind: i32,
    my_chips: i32,
) -> (String, i32) {
    let (act, amt, _) = decide_action_traced(
        hole,
        board,
        legal,
        to_call,
        pot,
        strategy_map,
        subgame_search,
        tracker,
        history,
        my_player,
        contributions,
        raises_this_round,
        strategy_index,
        solver,
        solver_ctx,
        active_opponents,
        big_blind,
        my_chips,
    );
    (act, amt)
}

#[allow(clippy::too_many_arguments)]
pub fn decide_action_traced(
    hole: &[Card; 2],
    board: &[Card],
    legal: &[String],
    to_call: i32,
    pot: i32,
    strategy_map: &HashMap<String, Vec<f64>>,
    subgame_search: bool,
    tracker: &OpponentTracker,
    history: &[RoundHistory; 4],
    my_player: usize,
    contributions: [i32; 2],
    raises_this_round: u8,
    strategy_index: Option<&StrategyLookupIndex>,
    solver: Option<&SubgameSolver>,
    solver_ctx: Option<&mut SubgameContext>,
    active_opponents: usize,
    big_blind: i32,
    my_chips: i32,
) -> (String, i32, DecisionTrace) {
    decide_action_traced_with_bet(
        hole,
        board,
        legal,
        to_call,
        pot,
        strategy_map,
        subgame_search,
        tracker,
        history,
        my_player,
        contributions,
        raises_this_round,
        strategy_index,
        solver,
        solver_ctx,
        active_opponents,
        big_blind,
        my_chips,
        0,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn decide_action_traced_with_bet(
    hole: &[Card; 2],
    board: &[Card],
    legal: &[String],
    to_call: i32,
    pot: i32,
    strategy_map: &HashMap<String, Vec<f64>>,
    subgame_search: bool,
    tracker: &OpponentTracker,
    history: &[RoundHistory; 4],
    my_player: usize,
    contributions: [i32; 2],
    raises_this_round: u8,
    strategy_index: Option<&StrategyLookupIndex>,
    solver: Option<&SubgameSolver>,
    solver_ctx: Option<&mut SubgameContext>,
    active_opponents: usize,
    big_blind: i32,
    my_chips: i32,
    current_bet: i32,
) -> (String, i32, DecisionTrace) {
    let bb = if big_blind > 0 { big_blind } else { BIG_BLIND };
    let active_opponents = active_opponents.max(1);
    let (to_call, pot) = if my_chips > 0 && to_call > my_chips {
        let uncallable = to_call - my_chips;
        let eff_pot = if pot > uncallable + my_chips {
            pot - uncallable
        } else {
            pot.max(my_chips)
        };
        (my_chips, eff_pot)
    } else {
        (to_call, pot)
    };
    let round = match board.len() {
        0 => 1,
        3 => 2,
        4 => 3,
        _ => 4,
    };

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

    let current_bucket = if round == 1 {
        preflop_bucket(hole[0], hole[1]).0
    } else {
        postflop_equity_bucket(hole, board)
    };

    /* Short-Stack Nash Push/Fold Engine:
     * When effective stack is <= 12 BB on preflop, delegates to unified OpponentTracker Nash Push/Fold. */
    if round == 1 && my_chips > 0 && my_chips <= bb * 12 {
        if let Some((short_act, reason)) = tracker.select_short_stack_preflop_action(
            current_bucket,
            to_call,
            pot,
            active_opponents,
            bb,
            my_chips,
            &legal_u8,
        ) {
            let total_pot = (pot + to_call).max(1);
            let pot_odds = to_call as f64 / total_pot as f64;
            let pot_odds_bps = (pot_odds.clamp(0.0, 1.0) * 1000.0).round() as u16;
            let tr = DecisionTrace {
                bucket: current_bucket,
                round: 1,
                pot_odds_bps,
                raw_top_action: short_act,
                final_action: short_act,
                guard_reasons: vec![reason],
            };
            match short_act {
                ALL_IN => {
                    if has("all_in") {
                        return ("all_in".to_string(), 0, tr);
                    } else if has("allin") {
                        return ("allin".to_string(), 0, tr);
                    } else if has("raise") {
                        return ("raise".to_string(), my_chips, tr);
                    } else if has("bet") {
                        return ("bet".to_string(), my_chips, tr);
                    } else if has("call") {
                        return ("call".to_string(), 0, tr);
                    } else {
                        return ("check".to_string(), 0, tr);
                    }
                }
                CALL_CHECK => {
                    if to_call == 0 && has("check") {
                        return ("check".to_string(), 0, tr);
                    } else if has("call") {
                        return ("call".to_string(), 0, tr);
                    } else if has("all_in") {
                        return ("all_in".to_string(), 0, tr);
                    } else if has("allin") {
                        return ("allin".to_string(), 0, tr);
                    } else if has("check") {
                        return ("check".to_string(), 0, tr);
                    }
                }
                _ => {
                    if has("fold") {
                        return ("fold".to_string(), 0, tr);
                    }
                }
            }
        }
    }

    /* 1. Real-time Subgame Search (Turn & River, or Flop for medium/big pots when Heads-Up 1v1):
     * Scale flop subgame trigger dynamically with big_blind (`bb * 8`). */
    let flop_trigger_pot = (bb * 8).max(1);
    let use_subgame = subgame_search
        && active_opponents == 1
        && (round >= 3 || (round == 2 && pot >= flop_trigger_pot));

    let (opp_vpip_opt, opp_pfr_opt) = if tracker.total_hands >= 6 {
        (Some(tracker.vpip_ratio()), Some(tracker.pfr_ratio()))
    } else {
        (None, None)
    };

    let raw_probs = if use_subgame {
        let default_solver = SubgameSolver::default();
        let active_solver = solver.unwrap_or(&default_solver);
        if let Some(ctx) = solver_ctx {
            active_solver.solve_with_stats(
                hole,
                board,
                round,
                history,
                my_player,
                contributions,
                raises_this_round,
                ctx,
                Some(strategy_map),
                opp_vpip_opt,
                opp_pfr_opt,
            )
        } else {
            let mut local_ctx = SubgameContext::new();
            active_solver.solve_with_stats(
                hole,
                board,
                round,
                history,
                my_player,
                contributions,
                raises_this_round,
                &mut local_ctx,
                Some(strategy_map),
                opp_vpip_opt,
                opp_pfr_opt,
            )
        }
    } else {
        /* 2. Abstract Strategy Model Lookup with O(1) Pre-Indexed Fallback */
        if let Some(p) = crate::cfr::fallback::lookup_strategy_hierarchical_owned(
            |k| strategy_map.get(k).cloned(),
            hole,
            board,
            round,
            history,
        ) {
            p
        } else if let Some(idx) = strategy_index {
            idx.get_blended_fallback_bb(hole, board, round, to_call, pot, &legal_u8, bb)
        } else {
            let fallback =
                get_holdem_fallback_strategy_bb(hole, board, round, to_call, pot, &legal_u8, bb);
            fallback.to_vec()
        }
    };

    /* 3. Action Selection via Purified Strategy Sampling with Multi-way, Opponent Exploitation, and All-in Defense */
    let mut rng = rand::thread_rng();
    let (chosen_u8, mut trace) = tracker.select_action_purified_traced_with_stack(
        &raw_probs,
        &legal_u8,
        current_bucket,
        round,
        to_call,
        pot,
        Some(hole),
        Some(board),
        active_opponents,
        bb,
        my_chips,
        &mut rng,
    );
    let mut chosen_idx = chosen_u8 as usize;

    /* 4. Postflop Low-SPR Commitment & River Turn-Aggressor Guards:
     * - On the River (round == 4) with < Trips (bucket < 65) when OOP (my_player == 1)
     *   and the opponent raised on the Turn, check to the Turn raiser instead of donk-betting into a shove!
     * - On Flop/Turn at low SPR (my_chips <= pot * 2) or bloated pot (pot >= bb * 8),
     *   never fire >= 33% pot bluffs with pure air (bucket < 11).
     * - Unconditional Pot-Commitment when pot_odds <= 12.5% (or <= 16.6% with bucket >= 15):
     *   never fold for 1-2 chips or tiny micro-stacks into a massive pot!
     * - Never call a meaningful postflop bet (to_call > bb, pot_odds > 16.6%) with pure air
     *   or bare gutshot (bucket < 15) if calling leaves < 50% of the new pot behind or commits >= 30% of stack. */
    if round == 4 && to_call == 0 && current_bucket < 65 && chosen_idx >= RAISE_MIN as usize {
        let turn_raises = history[2]
            .as_slice()
            .iter()
            .filter(|&&a| a >= RAISE_MIN)
            .count();
        if turn_raises >= 2 || (my_player == 1 && turn_raises >= 1) {
            chosen_idx = CALL_CHECK as usize;
            trace.guard_reasons.push("RIVER_CHECK_TO_TURN_AGGRESSOR");
        }
    }
    if to_call > 0 && chosen_idx == FOLD as usize && legal_u8.contains(&CALL_CHECK) {
        let new_pot = (pot + to_call).max(1);
        let is_postflop_micro_commit = round >= 2
            && (to_call * 8 <= new_pot
                || (to_call * 6 <= new_pot && current_bucket >= 15)
                || (my_chips > 0 && my_chips <= bb && pot >= bb * 4));
        let is_preflop_priced_in = round == 1
            && (to_call <= bb * 2 || preflop_strength_tier(current_bucket) <= 6)
            && to_call * 6 <= new_pot;
        if is_postflop_micro_commit || is_preflop_priced_in {
            chosen_idx = CALL_CHECK as usize;
            trace.guard_reasons.push("POT_COMMIT_DECISION_RESCUE");
        }
    }
    if round >= 2 && my_chips > 0 {
        let is_four_board_pair = round >= 3
            && (current_bucket == 28
                || current_bucket == 40
                || current_bucket == 50
                || current_bucket == 57);
        let is_bloated_flop_weak = round == 2
            && ((current_bucket < 15 && (my_chips <= pot * 2 || pot >= bb * 8))
                || ((28..=37).contains(&current_bucket) && pot >= bb * 10));
        if to_call == 0
            && chosen_idx >= RAISE_MIN as usize
            && (is_four_board_pair
                || is_bloated_flop_weak
                || (current_bucket < 11
                    && chosen_idx >= RAISE_THIRD_POT as usize
                    && (my_chips <= pot * 2 || pot >= bb * 8)))
        {
            chosen_idx = CALL_CHECK as usize;
            trace.guard_reasons.push("LOW_SPR_BLUFF_SUPPRESS");
        } else if to_call > bb
            && (current_bucket < 15 || current_bucket == 28)
            && chosen_idx == CALL_CHECK as usize
        {
            let remaining = my_chips - to_call;
            let new_pot = (pot + to_call).max(1);
            if to_call * 6 > new_pot && (remaining * 2 < new_pot || to_call * 10 >= my_chips * 3) {
                chosen_idx = FOLD as usize;
                trace.guard_reasons.push("LOW_SPR_AIR_CALL_FOLD");
            }
        }
    }
    trace.final_action = chosen_idx as u8;

    let is_wet = board.len() >= 3 && {
        let mut suits = [0u8; 4];
        for c in board {
            suits[c.suit as usize] += 1;
        }
        suits.iter().any(|&s| s >= 2)
    };

    if let Some((act, amt)) = map_action_index_full(
        chosen_idx,
        legal,
        to_call,
        pot,
        is_wet,
        round,
        bb,
        my_chips,
        current_bet,
    ) {
        return (act, amt, trace);
    }

    /* Safe default: Check (if to_call == 0) -> Call -> All-in (for nuts facing all-in) -> Check -> Fold */
    let (def_act, def_amt) = if to_call == 0 && has("check") {
        ("check".to_string(), 0)
    } else if has("call") {
        ("call".to_string(), 0)
    } else if to_call > 0 && round >= 2 && current_bucket >= 79 && has("all_in") {
        ("all_in".to_string(), 0)
    } else if to_call > 0 && round >= 2 && current_bucket >= 79 && has("allin") {
        ("allin".to_string(), 0)
    } else if has("check") {
        ("check".to_string(), 0)
    } else {
        ("fold".to_string(), 0)
    };
    (def_act, def_amt, trace)
}

/* Convenience wrapper for tests or callers that do not track detailed seats/chips */
#[allow(clippy::too_many_arguments)]
pub fn decide_action_simple(
    hole: &[Card; 2],
    board: &[Card],
    legal: &[String],
    to_call: i32,
    pot: i32,
    strategy_map: &HashMap<String, Vec<f64>>,
    subgame_search: bool,
    tracker: &OpponentTracker,
    history: &[RoundHistory; 4],
) -> (String, i32) {
    let my_contrib = ((pot - to_call) / 2).max(10);
    let opp_contrib = my_contrib + to_call;
    decide_action(
        hole,
        board,
        legal,
        to_call,
        pot,
        strategy_map,
        subgame_search,
        tracker,
        history,
        0,
        [my_contrib, opp_contrib],
        0,
        None,
        None,
        None,
    )
}
