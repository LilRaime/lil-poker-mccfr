/*! Opponent Archetype Heuristics and Action Sampling */
use std::fmt;
use std::str::FromStr;

use super::holdem::HoldemStrategy;
use crate::cfr::abstraction::{
    BUCKET_STRONG_DRAW_MIN, BUCKET_TOP_PAIR_MIN, BUCKET_TWO_PAIR_MIN, BUCKET_WEAK_PAIR_MIN,
    detect_draws, postflop_equity_bucket, preflop_bucket, preflop_strength_tier,
};
use crate::cfr::fallback::{FallbackRequest, get_holdem_fallback_strategy};
use crate::game::config::{BIG_BLIND, ROUND_FLOP, ROUND_PREFLOP, ROUND_TURN};
use crate::game::holdem::{
    ALL_IN, CALL_CHECK, FOLD as H_FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, TexasHoldemGame,
};
use rand::RngExt;
use rand::rngs::SmallRng;

pub fn pick_preferred_action(legal: &[u8], preferred: &[u8], rng: &mut SmallRng) -> u8 {
    let mut available = Vec::new();
    for &act in preferred {
        if legal.contains(&act) {
            available.push(act);
        }
    }
    if !available.is_empty() {
        available[rng.random_range(0..available.len())]
    } else {
        legal[rng.random_range(0..legal.len())]
    }
}

pub fn sample_from_probs(probs: &[f64], legal: &[u8], rng: &mut SmallRng) -> u8 {
    let raw: Vec<f64> = legal.iter().map(|&a| probs[a as usize].max(0.0)).collect();
    let sum: f64 = raw.iter().sum();
    if sum > 1e-12 {
        let r: f64 = rng.random();
        let mut cum = 0.0;
        for (i, &p) in raw.iter().enumerate() {
            cum += p / sum;
            if r < cum {
                return legal[i];
            }
        }
        legal.last().copied().unwrap_or(CALL_CHECK)
    } else {
        legal[rng.random_range(0..legal.len())]
    }
}

/** Typed simulated opponent archetype shared across engine, benchmarks, and CLIs */
#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum OpponentArchetype {
    #[default]
    #[value(name = "tag", alias = "tight_aggressive")]
    Tag = 0,
    #[value(name = "calling_station", aliases = ["station", "fish", "loose_passive"])]
    CallingStation = 1,
    #[value(name = "maniac", aliases = ["lag", "loose_aggressive"])]
    Maniac = 2,
    #[value(name = "rock", aliases = ["nit", "tight_passive"])]
    Rock = 3,
    #[value(name = "shove_bot", aliases = ["push_bot", "allin_bot"])]
    ShoveBot = 4,
    #[value(name = "trapper", alias = "check_raiser")]
    Trapper = 5,
    #[value(name = "overbet_aggro", aliases = ["overbet", "aggro_ob"])]
    OverbetAggro = 6,
    #[value(name = "self", alias = "model")]
    SelfPlay = 7,
    #[value(name = "random")]
    Random = 8,
}

impl OpponentArchetype {
    pub const GAUNTLET_ROSTER: [Self; 8] = [
        Self::Tag,
        Self::CallingStation,
        Self::Maniac,
        Self::ShoveBot,
        Self::Trapper,
        Self::Rock,
        Self::OverbetAggro,
        Self::SelfPlay,
    ];

    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tag => "tag",
            Self::CallingStation => "calling_station",
            Self::Maniac => "maniac",
            Self::Rock => "rock",
            Self::ShoveBot => "shove_bot",
            Self::Trapper => "trapper",
            Self::OverbetAggro => "overbet_aggro",
            Self::SelfPlay => "self",
            Self::Random => "random",
        }
    }

    #[inline]
    pub const fn banner_name(self) -> &'static str {
        match self {
            Self::Tag => "TAG",
            Self::CallingStation => "CALLING_STATION",
            Self::Maniac => "MANIAC",
            Self::Rock => "ROCK",
            Self::ShoveBot => "SHOVE_BOT",
            Self::Trapper => "TRAPPER",
            Self::OverbetAggro => "OVERBET_AGGRO",
            Self::SelfPlay => "SELF",
            Self::Random => "RANDOM",
        }
    }

    #[inline]
    pub const fn is_self_play(self) -> bool {
        matches!(self, Self::SelfPlay)
    }
}

impl fmt::Display for OpponentArchetype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OpponentArchetype {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "tag" | "tight_aggressive" => Ok(Self::Tag),
            "calling_station" | "station" | "fish" | "loose_passive" => Ok(Self::CallingStation),
            "maniac" | "lag" | "loose_aggressive" => Ok(Self::Maniac),
            "rock" | "nit" | "tight_passive" => Ok(Self::Rock),
            "shove_bot" | "push_bot" | "allin_bot" => Ok(Self::ShoveBot),
            "trapper" | "check_raiser" => Ok(Self::Trapper),
            "overbet_aggro" | "overbet" | "aggro_ob" => Ok(Self::OverbetAggro),
            "self" | "model" => Ok(Self::SelfPlay),
            "random" => Ok(Self::Random),
            other => Err(format!("Unknown opponent archetype: '{other}'")),
        }
    }
}

/** Sample an action according to opponent archetype */
pub fn sample_opponent_action(
    opp_archetype: OpponentArchetype,
    opp_strategy: Option<&HoldemStrategy>,
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

    match opp_archetype {
        OpponentArchetype::Tag => sample_tag_action(&ctx, rng),
        OpponentArchetype::CallingStation => sample_station_action(&ctx, rng),
        OpponentArchetype::Maniac => sample_maniac_action(&ctx, rng),
        OpponentArchetype::Rock => sample_nit_action(&ctx, rng),
        OpponentArchetype::ShoveBot => sample_shove_bot_action(&ctx, rng),
        OpponentArchetype::Trapper => sample_trapper_action(&ctx, rng),
        OpponentArchetype::OverbetAggro => sample_overbet_aggro_action(&ctx, rng),
        OpponentArchetype::SelfPlay => sample_model_action(&ctx, opp_strategy, rng),
        OpponentArchetype::Random => ctx.legal[rng.random_range(0..ctx.legal.len())],
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
    if c.game.round == ROUND_PREFLOP {
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
            if rng.random_bool(0.75) {
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
                if (has_fd || has_sd) && rng.random_bool(0.25) {
                    pick_preferred_action(c.legal, &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK], rng)
                } else {
                    CALL_CHECK
                }
            } else if c.opp_to_call <= BIG_BLIND * 3 {
                CALL_CHECK
            } else if rng.random_bool(0.60) {
                H_FOLD
            } else {
                CALL_CHECK
            }
        } else if c.opp_to_call == 0 {
            CALL_CHECK
        } else if rng.random_bool(0.10) && c.legal.contains(&RAISE_HALF_POT) {
            RAISE_HALF_POT
        } else {
            H_FOLD
        }
    }
}

fn sample_station_action(c: &OppActionCtx<'_>, rng: &mut SmallRng) -> u8 {
    if c.opp_to_call == 0 {
        if rng.random_bool(0.08) && c.legal.contains(&RAISE_MIN) {
            RAISE_MIN
        } else {
            CALL_CHECK
        }
    } else if c.game.round == ROUND_PREFLOP {
        let (idx, _) = preflop_bucket(c.hole[0], c.hole[1]);
        let tier = preflop_strength_tier(idx);
        if (c.opp_to_call > BIG_BLIND * 8 && tier >= 7 && rng.random_bool(0.85))
            || (c.opp_to_call > BIG_BLIND * 3 && tier == 8 && rng.random_bool(0.75))
        {
            H_FOLD
        } else {
            CALL_CHECK
        }
    } else {
        let bucket = postflop_equity_bucket(c.hole, c.board);
        if (c.opp_to_call > BIG_BLIND && bucket < BUCKET_STRONG_DRAW_MIN && rng.random_bool(0.35))
            || (c.opp_to_call > BIG_BLIND * 3 && bucket < 30 && rng.random_bool(0.60))
        {
            H_FOLD
        } else {
            CALL_CHECK
        }
    }
}

fn sample_maniac_action(c: &OppActionCtx<'_>, rng: &mut SmallRng) -> u8 {
    let r: f64 = rng.random();
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
    if c.game.round == ROUND_PREFLOP {
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
    if c.game.round == ROUND_PREFLOP {
        let (idx, _) = preflop_bucket(c.hole[0], c.hole[1]);
        let tier = preflop_strength_tier(idx);
        if tier <= 5 || rng.random_bool(0.35) {
            pick_preferred_action(c.legal, &[ALL_IN, RAISE_HALF_POT, RAISE_MIN], rng)
        } else if c.opp_to_call == 0 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    } else {
        let bucket = postflop_equity_bucket(c.hole, c.board);
        let (has_fd, has_sd) = detect_draws(c.hole, c.board);
        if bucket >= BUCKET_WEAK_PAIR_MIN || has_fd || has_sd || rng.random_bool(0.25) {
            pick_preferred_action(c.legal, &[ALL_IN, RAISE_HALF_POT, RAISE_MIN], rng)
        } else if c.opp_to_call == 0 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    }
}

fn sample_trapper_action(c: &OppActionCtx<'_>, rng: &mut SmallRng) -> u8 {
    if c.game.round == ROUND_PREFLOP {
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
            if c.game.round >= ROUND_TURN
                && (bucket >= BUCKET_TOP_PAIR_MIN || (bucket < 20 && rng.random_bool(0.30)))
            {
                pick_preferred_action(c.legal, &[RAISE_HALF_POT, ALL_IN, RAISE_THIRD_POT], rng)
            } else {
                CALL_CHECK
            }
        } else if bucket >= 36 || has_fd || has_sd || rng.random_bool(0.20) {
            pick_preferred_action(c.legal, &[RAISE_HALF_POT, ALL_IN, CALL_CHECK], rng)
        } else if bucket >= BUCKET_WEAK_PAIR_MIN && c.opp_to_call <= BIG_BLIND * 3 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    }
}

fn sample_overbet_aggro_action(c: &OppActionCtx<'_>, rng: &mut SmallRng) -> u8 {
    if c.game.round == ROUND_PREFLOP {
        let (idx, _) = preflop_bucket(c.hole[0], c.hole[1]);
        let tier = preflop_strength_tier(idx);
        if c.opp_to_call <= BIG_BLIND {
            if tier <= 6 || (tier == 7 && rng.random_bool(0.40)) {
                pick_preferred_action(c.legal, &[RAISE_MIN, RAISE_THIRD_POT, CALL_CHECK], rng)
            } else if c.opp_to_call == 0 {
                CALL_CHECK
            } else {
                H_FOLD
            }
        } else if tier <= 3 || ((5..=6).contains(&tier) && rng.random_bool(0.30)) {
            pick_preferred_action(
                c.legal,
                &[RAISE_HALF_POT, RAISE_THIRD_POT, ALL_IN, CALL_CHECK],
                rng,
            )
        } else if tier <= 5 && c.opp_to_call <= BIG_BLIND * 10 {
            CALL_CHECK
        } else {
            H_FOLD
        }
    } else {
        let bucket = postflop_equity_bucket(c.hole, c.board);
        let (has_fd, has_sd) = detect_draws(c.hole, c.board);
        if c.opp_to_call == 0 {
            if c.game.round == ROUND_FLOP {
                if rng.random_bool(0.75) && c.legal.contains(&RAISE_HALF_POT) {
                    RAISE_HALF_POT
                } else {
                    pick_preferred_action(
                        c.legal,
                        &[RAISE_THIRD_POT, RAISE_HALF_POT, CALL_CHECK],
                        rng,
                    )
                }
            } else if bucket >= BUCKET_TOP_PAIR_MIN
                || (c.game.round == ROUND_TURN && (has_fd || has_sd))
            {
                pick_preferred_action(
                    c.legal,
                    &[RAISE_HALF_POT, ALL_IN, RAISE_THIRD_POT, CALL_CHECK],
                    rng,
                )
            } else if rng.random_bool(0.38) {
                pick_preferred_action(
                    c.legal,
                    &[RAISE_HALF_POT, RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                    rng,
                )
            } else if c.game.round > ROUND_TURN && rng.random_bool(0.35) {
                pick_preferred_action(c.legal, &[RAISE_MIN, RAISE_THIRD_POT, CALL_CHECK], rng)
            } else {
                CALL_CHECK
            }
        } else if bucket >= BUCKET_TWO_PAIR_MIN
            || ((bucket >= BUCKET_TOP_PAIR_MIN || has_fd || has_sd) && c.game.round == ROUND_FLOP)
            || (c.game.round == ROUND_FLOP && rng.random_bool(0.30))
            || (c.game.round > ROUND_FLOP && rng.random_bool(0.20) && bucket < BUCKET_WEAK_PAIR_MIN)
        {
            pick_preferred_action(c.legal, &[RAISE_HALF_POT, ALL_IN, CALL_CHECK], rng)
        } else if bucket >= BUCKET_TOP_PAIR_MIN
            || ((bucket >= BUCKET_WEAK_PAIR_MIN || has_fd || has_sd)
                && c.opp_to_call <= (c.pot / 2).max(BIG_BLIND * 3))
        {
            CALL_CHECK
        } else {
            H_FOLD
        }
    }
}

fn sample_model_action(
    c: &OppActionCtx<'_>,
    opp_strategy: Option<&HoldemStrategy>,
    rng: &mut SmallRng,
) -> u8 {
    if let Some(strat) = opp_strategy {
        let eff_stack_bb = (c.game.stack_size / BIG_BLIND.max(1)).max(1);
        let probs = if let Some(s) = crate::cfr::fallback::lookup_strategy_hierarchical_with_stack(
            |k| strat.get(k),
            c.hole,
            c.board,
            c.game.round,
            &c.game.history,
            eff_stack_bb,
        ) {
            s.to_vec()
        } else {
            get_holdem_fallback_strategy(&FallbackRequest {
                hole: *c.hole,
                board: c.board,
                round: c.game.round,
                to_call: c.opp_to_call,
                pot: c.pot,
                legal: c.legal,
                ..Default::default()
            })
            .to_vec()
        };
        sample_from_probs(&probs, c.legal, rng)
    } else {
        let probs = get_holdem_fallback_strategy(&FallbackRequest {
            hole: *c.hole,
            board: c.board,
            round: c.game.round,
            to_call: c.opp_to_call,
            pot: c.pot,
            legal: c.legal,
            ..Default::default()
        });
        sample_from_probs(&probs, c.legal, rng)
    }
}
