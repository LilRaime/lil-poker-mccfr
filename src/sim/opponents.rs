/* Opponent Archetype Heuristics and Action Sampling */
use super::holdem::HoldemStrategy;
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
    let ctx = OppActionCtx {
        game,
        hole: &game.hole[opp_player],
        board: &game.board,
        legal: &legal,
        opp_to_call: (game.contributions[1 - opp_player] - game.contributions[opp_player]).max(0),
        pot: game.contributions[0] + game.contributions[1],
    };

    match opp_archetype.to_lowercase().as_str() {
        "tag" | "tight_aggressive" => sample_tag_action(&ctx, rng),
        "calling_station" | "station" | "fish" | "loose_passive" => {
            sample_station_action(&ctx, rng)
        }
        "maniac" | "lag" | "loose_aggressive" => sample_maniac_action(&ctx, rng),
        "rock" | "nit" | "tight_passive" => sample_nit_action(&ctx, rng),
        "shove_bot" | "push_bot" | "allin_bot" => sample_shove_bot_action(&ctx, rng),
        "trapper" | "check_raiser" => sample_trapper_action(&ctx, rng),
        "self" | "model" => sample_model_action(&ctx, opp_strategy, rng),
        _ => ctx.legal[rng.gen_range(0..ctx.legal.len())],
    }
}

struct OppActionCtx<'a> {
    game: &'a TexasHoldemGame,
    hole: &'a [crate::game::holdem::Card; 2],
    board: &'a [crate::game::holdem::Card],
    legal: &'a [u8],
    opp_to_call: i32,
    pot: i32,
}

fn sample_tag_action(c: &OppActionCtx<'_>, rng: &mut SmallRng) -> u8 {
    if c.game.round == 1 {
        let (idx, _) = preflop_bucket(c.hole[0], c.hole[1]);
        let tier = preflop_strength_tier(idx);
        if tier <= 2 {
            pick_preferred_action(
                c.legal,
                &[RAISE_HALF_POT, RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                rng,
            )
        } else if tier == 3 {
            if c.opp_to_call == 0 {
                pick_preferred_action(c.legal, &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK], rng)
            } else if c.opp_to_call <= BIG_BLIND * 2 {
                pick_preferred_action(c.legal, &[CALL_CHECK, RAISE_MIN], rng)
            } else {
                pick_preferred_action(c.legal, &[CALL_CHECK, H_FOLD], rng)
            }
        } else if tier == 4 {
            if c.opp_to_call == 0 {
                CALL_CHECK
            } else if c.opp_to_call <= BIG_BLIND {
                pick_preferred_action(c.legal, &[CALL_CHECK, H_FOLD], rng)
            } else {
                H_FOLD
            }
        } else if c.opp_to_call == 0 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    } else {
        let bucket = postflop_equity_bucket(c.hole, c.board);
        let (has_fd, has_sd) = detect_draws(c.hole, c.board);
        if bucket >= 38 {
            if rng.gen_bool(0.75) {
                pick_preferred_action(
                    c.legal,
                    &[RAISE_HALF_POT, RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                    rng,
                )
            } else {
                CALL_CHECK
            }
        } else if bucket >= 30 || has_fd || has_sd {
            if c.opp_to_call == 0 {
                if (has_fd || has_sd) && rng.gen_bool(0.25) {
                    pick_preferred_action(c.legal, &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK], rng)
                } else {
                    CALL_CHECK
                }
            } else if c.opp_to_call <= BIG_BLIND * 3 {
                CALL_CHECK
            } else if rng.gen_bool(0.60) {
                H_FOLD
            } else {
                CALL_CHECK
            }
        } else if c.opp_to_call == 0 {
            CALL_CHECK
        } else if rng.gen_bool(0.10) && c.legal.contains(&RAISE_HALF_POT) {
            RAISE_HALF_POT
        } else {
            H_FOLD
        }
    }
}

fn sample_station_action(c: &OppActionCtx<'_>, rng: &mut SmallRng) -> u8 {
    if c.opp_to_call == 0 {
        if rng.gen_bool(0.08) && c.legal.contains(&RAISE_MIN) {
            RAISE_MIN
        } else {
            CALL_CHECK
        }
    } else if c.game.round == 1 {
        let (idx, _) = preflop_bucket(c.hole[0], c.hole[1]);
        let tier = preflop_strength_tier(idx);
        if (c.opp_to_call > BIG_BLIND * 8 && tier >= 7 && rng.gen_bool(0.85))
            || (c.opp_to_call > BIG_BLIND * 3 && tier == 8 && rng.gen_bool(0.75))
        {
            H_FOLD
        } else {
            CALL_CHECK
        }
    } else {
        let bucket = postflop_equity_bucket(c.hole, c.board);
        if (c.opp_to_call > BIG_BLIND && bucket < 15 && rng.gen_bool(0.35))
            || (c.opp_to_call > BIG_BLIND * 3 && bucket < 30 && rng.gen_bool(0.60))
        {
            H_FOLD
        } else {
            CALL_CHECK
        }
    }
}

fn sample_maniac_action(c: &OppActionCtx<'_>, rng: &mut SmallRng) -> u8 {
    let r: f64 = rng.gen();
    if c.opp_to_call == 0 {
        if r < 0.40 && c.legal.contains(&RAISE_HALF_POT) {
            RAISE_HALF_POT
        } else if r < 0.65 && c.legal.contains(&RAISE_THIRD_POT) {
            RAISE_THIRD_POT
        } else if r < 0.80 && c.legal.contains(&RAISE_MIN) {
            RAISE_MIN
        } else {
            CALL_CHECK
        }
    } else if r < 0.35 && c.legal.contains(&RAISE_HALF_POT) {
        RAISE_HALF_POT
    } else if r < 0.50 && c.legal.contains(&ALL_IN) {
        ALL_IN
    } else if r < 0.85 {
        CALL_CHECK
    } else {
        H_FOLD
    }
}

fn sample_nit_action(c: &OppActionCtx<'_>, rng: &mut SmallRng) -> u8 {
    if c.game.round == 1 {
        let (idx, _) = preflop_bucket(c.hole[0], c.hole[1]);
        let tier = preflop_strength_tier(idx);
        if tier == 1 {
            pick_preferred_action(c.legal, &[RAISE_MIN, RAISE_THIRD_POT, CALL_CHECK], rng)
        } else if (tier == 2 && c.opp_to_call <= BIG_BLIND) || c.opp_to_call == 0 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    } else {
        let bucket = postflop_equity_bucket(c.hole, c.board);
        if bucket >= 46 {
            pick_preferred_action(c.legal, &[RAISE_HALF_POT, RAISE_THIRD_POT, CALL_CHECK], rng)
        } else if (bucket >= 36 && c.opp_to_call <= BIG_BLIND * 2) || c.opp_to_call == 0 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    }
}

fn sample_shove_bot_action(c: &OppActionCtx<'_>, rng: &mut SmallRng) -> u8 {
    if c.game.round == 1 {
        let (idx, _) = preflop_bucket(c.hole[0], c.hole[1]);
        let tier = preflop_strength_tier(idx);
        if tier <= 5 || rng.gen_bool(0.35) {
            pick_preferred_action(c.legal, &[ALL_IN, RAISE_HALF_POT, RAISE_MIN], rng)
        } else if c.opp_to_call == 0 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    } else {
        let bucket = postflop_equity_bucket(c.hole, c.board);
        let (has_fd, has_sd) = detect_draws(c.hole, c.board);
        if bucket >= 28 || has_fd || has_sd || rng.gen_bool(0.25) {
            pick_preferred_action(c.legal, &[ALL_IN, RAISE_HALF_POT, RAISE_MIN], rng)
        } else if c.opp_to_call == 0 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    }
}

fn sample_trapper_action(c: &OppActionCtx<'_>, rng: &mut SmallRng) -> u8 {
    if c.game.round == 1 {
        let (idx, _) = preflop_bucket(c.hole[0], c.hole[1]);
        let tier = preflop_strength_tier(idx);
        if tier <= 3 {
            if c.opp_to_call == 0 {
                CALL_CHECK
            } else {
                pick_preferred_action(c.legal, &[CALL_CHECK, RAISE_MIN], rng)
            }
        } else if (tier <= 5 && c.opp_to_call <= BIG_BLIND * 2) || c.opp_to_call == 0 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    } else {
        let bucket = postflop_equity_bucket(c.hole, c.board);
        let (has_fd, has_sd) = detect_draws(c.hole, c.board);
        if c.opp_to_call == 0 {
            if c.game.round >= 3 && (bucket >= 40 || (bucket < 20 && rng.gen_bool(0.30))) {
                pick_preferred_action(c.legal, &[RAISE_HALF_POT, ALL_IN, RAISE_THIRD_POT], rng)
            } else {
                CALL_CHECK
            }
        } else if bucket >= 36 || has_fd || has_sd || rng.gen_bool(0.20) {
            pick_preferred_action(c.legal, &[RAISE_HALF_POT, ALL_IN, CALL_CHECK], rng)
        } else if bucket >= 28 && c.opp_to_call <= BIG_BLIND * 3 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    }
}

fn sample_model_action(
    c: &OppActionCtx<'_>,
    opp_strategy: &Option<HoldemStrategy>,
    rng: &mut SmallRng,
) -> u8 {
    if let Some(strat) = opp_strategy {
        let probs = if let Some(s) = crate::cfr::fallback::lookup_strategy_hierarchical(
            |k| strat.get(k),
            c.hole,
            c.board,
            c.game.round,
            &c.game.history,
        ) {
            s.to_vec()
        } else {
            get_holdem_fallback_strategy(
                c.hole,
                c.board,
                c.game.round,
                c.opp_to_call,
                c.pot,
                c.legal,
            )
            .to_vec()
        };
        sample_from_probs(&probs, c.legal, rng)
    } else {
        let probs = get_holdem_fallback_strategy(
            c.hole,
            c.board,
            c.game.round,
            c.opp_to_call,
            c.pot,
            c.legal,
        );
        sample_from_probs(&probs, c.legal, rng)
    }
}
