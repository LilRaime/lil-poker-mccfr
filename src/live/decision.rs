/* Real-time Decision Engine for Live Poker Client */
use std::collections::HashMap;

use super::protocol::map_action_index_round;
use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};
use crate::cfr::fallback::{get_holdem_fallback_strategy, StrategyLookupIndex};
use crate::cfr::opponent_model::OpponentTracker;
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
    if has("check") || has("call") {
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

    /* 1. Real-time Subgame Search (Turn & River, or Flop for big pots) */
    let raw_probs =
        if subgame_search && (round >= 3 || (round == 2 && pot >= SUBGAME_FLOP_TRIGGER_POT)) {
            let default_solver = SubgameSolver::default();
            let active_solver = solver.unwrap_or(&default_solver);
            if let Some(ctx) = solver_ctx {
                active_solver.solve_with_context_and_strategy(
                    hole,
                    board,
                    round,
                    history,
                    my_player,
                    contributions,
                    raises_this_round,
                    ctx,
                    Some(strategy_map),
                )
            } else {
                active_solver.solve_with_state(
                    hole,
                    board,
                    round,
                    history,
                    my_player,
                    contributions,
                    raises_this_round,
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
                idx.get_blended_fallback(hole, board, round, to_call, pot, &legal_u8)
            } else {
                let fallback =
                    get_holdem_fallback_strategy(hole, board, round, to_call, pot, &legal_u8);
                fallback.to_vec()
            }
        };

    /* 3. Action Selection via Purified Strategy Sampling with Opponent Exploitation and All-in Defense */
    let mut rng = rand::thread_rng();
    let chosen_idx = tracker.select_action_purified_context(
        &raw_probs,
        &legal_u8,
        current_bucket,
        round,
        to_call,
        pot,
        Some(hole),
        Some(board),
        &mut rng,
    ) as usize;

    let is_wet = board.len() >= 3 && {
        let mut suits = [0u8; 4];
        for c in board {
            suits[c.suit as usize] += 1;
        }
        suits.iter().any(|&s| s >= 2)
    };

    if let Some(res) = map_action_index_round(chosen_idx, legal, to_call, pot, is_wet, round) {
        return res;
    }

    /* Safe default: Check (if to_call == 0) -> Call -> Check -> Fold */
    if to_call == 0 && has("check") {
        ("check".to_string(), 0)
    } else if has("call") {
        ("call".to_string(), 0)
    } else if has("check") {
        ("check".to_string(), 0)
    } else {
        ("fold".to_string(), 0)
    }
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
