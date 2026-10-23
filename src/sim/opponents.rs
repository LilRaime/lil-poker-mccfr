/* Opponent Archetype Heuristics and Action Sampling */
use super::holdem::{find_strategy_or_fallback, HoldemStrategy};
use crate::cfr::abstraction::{
    detect_draws, postflop_equity_bucket, preflop_bucket, preflop_strength_tier,
};
use crate::cfr::fallback::get_holdem_fallback_strategy;
use crate::game::config::BIG_BLIND;
use crate::game::holdem::{
    TexasHoldemGame, ALL_IN, CALL_CHECK, FOLD as H_FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use rand::rngs::SmallRng;
use rand::Rng;

pub fn pick_preferred_action(legal: &[u8], preferred: &[u8], rng: &mut SmallRng) -> u8 {
    let mut available = Vec::new();
    for &act in preferred {
        if legal.contains(&act) {
            available.push(act);
        }
    }
    if !available.is_empty() {
        available[rng.gen_range(0..available.len())]
    } else {
        legal[rng.gen_range(0..legal.len())]
    }
}

pub fn sample_from_probs(probs: &[f64], legal: &[u8], rng: &mut SmallRng) -> u8 {
    let raw: Vec<f64> = legal.iter().map(|&a| probs[a as usize].max(0.0)).collect();
    let sum: f64 = raw.iter().sum();
    if sum > 1e-12 {
        let r: f64 = rng.gen();
        let mut cum = 0.0;
        for (i, &p) in raw.iter().enumerate() {
            cum += p / sum;
            if r < cum {
                return legal[i];
            }
        }
        *legal.last().unwrap()
    } else {
        legal[rng.gen_range(0..legal.len())]
    }
}

/* Sample an action according to opponent archetype */
pub fn sample_opponent_action(
    opp_archetype: &str,
    opp_strategy: &Option<HoldemStrategy>,
    game: &TexasHoldemGame,
    opp_player: usize,
    rng: &mut SmallRng,
) -> u8 {
    let legal = game.legal_actions();
    if legal.is_empty() {
        return CALL_CHECK;
    }
    let opp_to_call = (game.contributions[1 - opp_player] - game.contributions[opp_player]).max(0);
    let pot = game.contributions[0] + game.contributions[1];
    let hole = &game.hole[opp_player];
    let board = &game.board;

    match opp_archetype.to_lowercase().as_str() {
        "tag" | "tight_aggressive" => {
            if game.round == 1 {
                let (idx, _) = preflop_bucket(hole[0], hole[1]);
                let tier = preflop_strength_tier(idx);
                if tier <= 2 {
                    pick_preferred_action(
                        &legal,
                        &[RAISE_HALF_POT, RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                        rng,
                    )
                } else if tier == 3 {
                    if opp_to_call == 0 {
                        pick_preferred_action(
                            &legal,
                            &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                            rng,
                        )
                    } else if opp_to_call <= BIG_BLIND * 2 {
                        pick_preferred_action(&legal, &[CALL_CHECK, RAISE_MIN], rng)
                    } else {
                        pick_preferred_action(&legal, &[CALL_CHECK, H_FOLD], rng)
                    }
                } else if tier == 4 {
                    if opp_to_call == 0 {
                        CALL_CHECK
                    } else if opp_to_call <= BIG_BLIND {
                        pick_preferred_action(&legal, &[CALL_CHECK, H_FOLD], rng)
                    } else {
                        H_FOLD
                    }
                } else {
                    if opp_to_call == 0 {
                        CALL_CHECK
                    } else {
                        H_FOLD
                    }
                }
            } else {
                let bucket = postflop_equity_bucket(hole, board);
                let (has_fd, has_sd) = detect_draws(hole, board);
                if bucket >= 38 {
                    if rng.gen_bool(0.75) {
                        pick_preferred_action(
                            &legal,
                            &[RAISE_HALF_POT, RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                            rng,
                        )
                    } else {
                        CALL_CHECK
                    }
                } else if bucket >= 30 || has_fd || has_sd {
                    if opp_to_call == 0 {
                        if (has_fd || has_sd) && rng.gen_bool(0.25) {
                            pick_preferred_action(
                                &legal,
                                &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                                rng,
                            )
                        } else {
                            CALL_CHECK
                        }
                    } else if opp_to_call <= BIG_BLIND * 3 {
                        CALL_CHECK
                    } else {
                        if rng.gen_bool(0.60) {
                            H_FOLD
                        } else {
                            CALL_CHECK
                        }
                    }
                } else {
                    if opp_to_call == 0 {
                        CALL_CHECK
                    } else {
                        if rng.gen_bool(0.10) && legal.contains(&RAISE_HALF_POT) {
                            RAISE_HALF_POT
                        } else {
                            H_FOLD
                        }
                    }
                }
            }
        }
        "calling_station" | "station" | "fish" | "loose_passive" => {
            if opp_to_call == 0 {
                if rng.gen_bool(0.08) && legal.contains(&RAISE_MIN) {
                    RAISE_MIN
                } else {
                    CALL_CHECK
                }
            } else {
                let bucket = if game.round == 1 {
                    35
                } else {
                    postflop_equity_bucket(hole, board)
                };
                if (opp_to_call > BIG_BLIND && bucket < 15 && rng.gen_bool(0.35))
                    || (opp_to_call > BIG_BLIND * 3 && bucket < 30 && rng.gen_bool(0.60))
                {
                    H_FOLD
                } else {
                    CALL_CHECK
                }
            }
        }
        "maniac" | "lag" | "loose_aggressive" => {
            if opp_to_call == 0 {
                let r: f64 = rng.gen();
                if r < 0.40 && legal.contains(&RAISE_HALF_POT) {
                    RAISE_HALF_POT
                } else if r < 0.65 && legal.contains(&RAISE_THIRD_POT) {
                    RAISE_THIRD_POT
                } else if r < 0.80 && legal.contains(&RAISE_MIN) {
                    RAISE_MIN
                } else {
                    CALL_CHECK
                }
            } else {
                let r: f64 = rng.gen();
                if r < 0.35 && legal.contains(&RAISE_HALF_POT) {
                    RAISE_HALF_POT
                } else if r < 0.50 && legal.contains(&ALL_IN) {
                    ALL_IN
                } else if r < 0.85 {
                    CALL_CHECK
                } else {
                    H_FOLD
                }
            }
        }
        "rock" | "nit" | "tight_passive" => {
            if game.round == 1 {
                let (idx, _) = preflop_bucket(hole[0], hole[1]);
                let tier = preflop_strength_tier(idx);
                if tier == 1 {
                    pick_preferred_action(&legal, &[RAISE_MIN, RAISE_THIRD_POT, CALL_CHECK], rng)
                } else if tier == 2 && opp_to_call <= BIG_BLIND {
                    CALL_CHECK
                } else {
                    if opp_to_call == 0 {
                        CALL_CHECK
                    } else {
                        H_FOLD
                    }
                }
            } else {
                let bucket = postflop_equity_bucket(hole, board);
                if bucket >= 46 {
                    pick_preferred_action(
                        &legal,
                        &[RAISE_HALF_POT, RAISE_THIRD_POT, CALL_CHECK],
                        rng,
                    )
                } else if bucket >= 36 && opp_to_call <= BIG_BLIND * 2 {
                    CALL_CHECK
                } else {
                    if opp_to_call == 0 {
                        CALL_CHECK
                    } else {
                        H_FOLD
                    }
                }
            }
        }
        "self" | "model" => {
            if let Some(strat) = opp_strategy {
                let probs = if let Some(s) = crate::cfr::fallback::lookup_strategy_hierarchical(
                    |k| strat.get(k),
                    hole,
                    board,
                    game.round,
                    &game.history,
                ) {
                    s.to_vec()
                } else {
                    find_strategy_or_fallback(
                        strat,
                        hole,
                        board,
                        game.round,
                        opp_to_call,
                        pot,
                        &legal,
                    )
                };
                sample_from_probs(&probs, &legal, rng)
            } else {
                let probs =
                    get_holdem_fallback_strategy(hole, board, game.round, opp_to_call, pot, &legal);
                sample_from_probs(&probs, &legal, rng)
            }
        }
        _ => legal[rng.gen_range(0..legal.len())],
    }
}
