/* Real-time Decision Engine for Live Poker Client */
use std::collections::HashMap;

use super::protocol::map_action_index;
use crate::cfr::abstraction::{
    get_holdem_infoset_key, get_holdem_infoset_key_rich, postflop_equity_bucket, preflop_bucket,
};
use crate::cfr::fallback::get_holdem_fallback_strategy;
use crate::cfr::opponent_model::OpponentTracker;
use crate::cfr::subgame::SubgameSolver;
use crate::game::config::SUBGAME_FLOP_TRIGGER_POT;
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
        legal_u8.push(0);
    }
    if has("check") || has("call") {
        legal_u8.push(1);
    }
    if has("raise") || has("bet") {
        legal_u8.push(2);
        legal_u8.push(3);
        legal_u8.push(4);
    }
    if has("allin") {
        legal_u8.push(5);
    }
    if legal_u8.is_empty() {
        if has("check") || has("call") {
            legal_u8.push(1);
        } else if has("fold") {
            legal_u8.push(0);
        } else if has("allin") {
            legal_u8.push(5);
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
            let solver = SubgameSolver::new(2500);
            let my_contrib = (pot / 2).max(10);
            let opp_contrib = my_contrib + to_call;
            solver.solve_with_state(hole, board, round, history, 0, [my_contrib, opp_contrib], 0)
        } else {
            /* 2. Abstract Strategy Model Lookup with Fallback (rich key first, then exact key) */
            let rich_key = get_holdem_infoset_key_rich(hole, board, round, history);
            let exact_key = get_holdem_infoset_key(hole, board, round, history);
            if let Some(p) = strategy_map
                .get(&rich_key)
                .or_else(|| strategy_map.get(&exact_key))
            {
                p.clone()
            } else {
                let prefix = if round == 1 {
                    let (_, name) = preflop_bucket(hole[0], hole[1]);
                    format!("P:{}/", name)
                } else {
                    let bucket = postflop_equity_bucket(hole, board);
                    let r_code = match round {
                        2 => "F",
                        3 => "T",
                        4 => "R",
                        _ => "X",
                    };
                    format!("{}:B{:02}", r_code, bucket)
                };

                let matches: Vec<&Vec<f64>> = strategy_map
                    .iter()
                    .filter(|(k, _)| {
                        if round == 1 {
                            k.starts_with(&prefix)
                        } else {
                            k.contains(&prefix)
                        }
                    })
                    .map(|(_, v)| v)
                    .collect();

                let fallback =
                    get_holdem_fallback_strategy(hole, board, round, to_call, pot, &legal_u8);

                if !matches.is_empty() {
                    let n = matches.len() as f64;
                    let mut avg = [0.0f64; 6];
                    for vec in matches {
                        for (i, &val) in vec.iter().enumerate().take(6) {
                            avg[i] += val / n;
                        }
                    }
                    let mut blended = vec![0.0f64; 6];
                    for &a in &legal_u8 {
                        let idx = a as usize;
                        if idx < 6 {
                            blended[idx] = 0.55 * avg[idx] + 0.45 * fallback[idx];
                        }
                    }
                    blended
                } else {
                    fallback.to_vec()
                }
            }
        };

    /* 3. Action Selection via Purified Strategy Sampling with Opponent Exploitation and All-in Defense */
    let mut rng = rand::thread_rng();
    let chosen_idx = tracker.select_action_purified(
        &raw_probs,
        &legal_u8,
        current_bucket,
        round == 1,
        to_call,
        &mut rng,
    ) as usize;

    let is_wet = board.len() >= 3 && {
        let mut suits = [0u8; 4];
        for c in board {
            suits[c.suit as usize] += 1;
        }
        suits.iter().any(|&s| s >= 2)
    };

    if let Some(res) = map_action_index(chosen_idx, legal, to_call, pot, is_wet) {
        return res;
    }

    /* Safe default: Check -> Call -> Fold */
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
