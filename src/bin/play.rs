/* Interactive Poker CLI & Game Simulator for Leduc Hold'em and 52-Card Texas Hold'em.
 * Usage:
 *   play --game holdem --mode episodes --hands 7
 *   play --game leduc  --mode episodes --hands 7
 */

use clap::Parser;
use lil_poker_mccfr::cfr::abstraction::get_holdem_infoset_key;
use lil_poker_mccfr::game::card::Card;
use lil_poker_mccfr::game::holdem::{
    TexasHoldemGame, ALL_52_CARDS, ALL_IN, CALL_CHECK, FOLD as H_FOLD, RAISE_HALF_POT, RAISE_MIN,
    RAISE_THIRD_POT,
};
use lil_poker_mccfr::game::leduc::{LeducGame, CALL, FOLD, RAISE};
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, Write};
use std::thread::sleep;
use std::time::Duration;

/* Leduc strategy: 3 actions [fold, call, raise] */
type LeducStrategy = HashMap<String, [f64; 3]>;
/* Hold'em strategy: 6 actions [fold, call_check, raise_min, raise_third_pot, raise_half_pot, all_in] */
type HoldemStrategy = HashMap<String, [f64; 6]>;

/* Keep backward-compat alias used in Leduc helpers. */
type Strategy = LeducStrategy;

#[derive(Default, Clone, Debug)]
struct SimStats {
    hands: u64,
    wins: u64,
    ties: u64,
    losses: u64,
    total_chips: f64,
    sq_chips: f64,
}

impl SimStats {
    fn record(&mut self, payoff: f64) {
        self.hands += 1;
        if payoff > 0.0 {
            self.wins += 1;
        } else if payoff == 0.0 {
            self.ties += 1;
        } else {
            self.losses += 1;
        }
        self.total_chips += payoff;
        self.sq_chips += payoff * payoff;
    }

    fn win_pct(&self) -> f64 {
        if self.hands == 0 {
            0.0
        } else {
            (self.wins as f64 / self.hands as f64) * 100.0
        }
    }

    fn loss_pct(&self) -> f64 {
        if self.hands == 0 {
            0.0
        } else {
            (self.losses as f64 / self.hands as f64) * 100.0
        }
    }

    fn tie_pct(&self) -> f64 {
        if self.hands == 0 {
            0.0
        } else {
            (self.ties as f64 / self.hands as f64) * 100.0
        }
    }

    fn avg_chips(&self) -> f64 {
        if self.hands == 0 {
            0.0
        } else {
            self.total_chips / self.hands as f64
        }
    }

    fn se_chips(&self) -> f64 {
        if self.hands <= 1 {
            return 0.0;
        }
        let n = self.hands as f64;
        let mean = self.avg_chips();
        let var = (self.sq_chips / n) - (mean * mean);
        (var.max(0.0) / n).sqrt()
    }
}

#[derive(Parser, Debug)]
#[command(about = "Play or Watch Poker CLI Simulation (Leduc or 52-card Texas Hold'em)")]
struct Args {
    /* Strategy JSON file for bot */
    #[arg(short, long, default_value = "models/leduc_strategy.json")]
    strategy: String,

    /* Game variant: 'holdem' (full 52-card) or 'leduc' (6-card) */
    #[arg(short, long, default_value = "holdem")]
    game: String,

    /* Game mode: 'episodes', 'watch', or 'human' */
    #[arg(short, long, default_value = "episodes")]
    mode: String,

    /* Number of hands/episodes to simulate */
    #[arg(short = 'n', long, default_value_t = 7)]
    hands: u64,

    /* Delay in milliseconds between steps in watch mode */
    #[arg(short, long, default_value_t = 0)]
    delay: u64,

    /* Enable Real-Time Subgame Search (Subgame Solving) on Turn/River */
    #[arg(long, default_value_t = false)]
    subgame_search: bool,

    /* Player seat: 'alternate' (default, alternates SB and BB), '0' (SB), or '1' (BB) */
    #[arg(long, default_value = "alternate")]
    player: String,

    /* Opponent archetype: 'random', 'tag', 'calling_station', 'maniac', 'rock', 'self' */
    #[arg(long, alias = "opp-archetype", default_value = "random")]
    opponent: String,

    /* Optional strategy JSON file for opponent (e.g. for model-vs-model benchmarks) */
    #[arg(long)]
    opponent_strategy: Option<String>,

    /* Duplicate Poker mode: each deal is played twice swapping seats to eliminate card luck variance */
    #[arg(long, default_value_t = false)]
    duplicate: bool,
}

fn main() {
    let args = Args::parse();

    if args.game == "holdem" {
        /* Default holdem strategy path if the user didn't override it. */
        let holdem_strategy_path = if args.strategy == "models/leduc_strategy.json" {
            "models/holdem_abstract_strategy.json".to_string()
        } else {
            args.strategy.clone()
        };
        run_holdem_episodes_log_mode(
            &holdem_strategy_path,
            args.hands,
            args.delay,
            args.subgame_search,
            &args.player,
            &args.opponent,
            args.opponent_strategy.as_deref(),
            args.duplicate,
        );
        return;
    }

    let strategy = load_strategy(&args.strategy);

    match args.mode.as_str() {
        "episodes" | "episode" | "rl" | "log" => {
            run_episodes_log_mode(&strategy, args.hands, args.delay, &args.player);
        }
        "watch" | "sim" | "simulation" => {
            println!("============================================================");
            println!("        ♠️  lil-poker-mccfr: Leduc Poker Game CLI  ♥️        ");
            println!("============================================================");
            println!(
                "Loaded Strategy: {} infosets from {}",
                strategy.len(),
                args.strategy
            );
            run_watch_mode(&strategy, args.hands, args.delay);
        }
        _ => {
            println!("============================================================");
            println!("        ♠️  lil-poker-mccfr: Leduc Poker Game CLI  ♥️        ");
            println!("============================================================");
            println!(
                "Loaded Strategy: {} infosets from {}",
                strategy.len(),
                args.strategy
            );
            run_human_mode(&strategy, args.hands);
        }
    }
}

/* Hold'em Strategy Loader (supports both legacy 4-action and modern 6-action formats) */
fn load_holdem_strategy(path: &str) -> Option<HoldemStrategy> {
    let data = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&data).ok()?;
    let map = json.as_object()?;
    Some(
        map.iter()
            .filter_map(|(k, v)| {
                let arr = v.as_array()?;
                let p = |i: usize| arr.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0);
                let strat = if arr.len() >= 6 {
                    [p(0), p(1), p(2), p(3), p(4), p(5)]
                } else {
                    /* Map legacy 4-action entries [fold, call, raise_min, raise_half_pot] */
                    [p(0), p(1), p(2), 0.0, p(3), 0.0]
                };
                Some((k.clone(), strat))
            })
            .collect(),
    )
}

/* Sample an action from a Hold'em strategy entry for the current game state. */
use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
use lil_poker_mccfr::cfr::subgame::SubgameSolver;
use lil_poker_mccfr::game::holdem::Card as HCard;

fn find_strategy_or_fallback(
    strategy: &HoldemStrategy,
    hole: &[HCard; 2],
    board: &[HCard],
    round: u8,
    to_call: i32,
    pot: i32,
    legal: &[u8],
) -> Vec<f64> {
    use lil_poker_mccfr::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};
    use lil_poker_mccfr::cfr::fallback::get_holdem_fallback_strategy;

    let fallback = get_holdem_fallback_strategy(hole, board, round, to_call, pot, legal);

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
        format!("{}:B{:02}/", r_code, bucket)
    };

    let matches: Vec<&[f64; 6]> = strategy
        .iter()
        .filter(|(k, _)| k.starts_with(&prefix))
        .map(|(_, v)| v)
        .collect();

    if !matches.is_empty() {
        let n = matches.len() as f64;
        let mut model_avg = [0.0f64; 6];
        for s in &matches {
            for i in 0..6 {
                model_avg[i] += s[i] / n;
            }
        }
        let mut blended = [0.0f64; 6];
        let mut sum = 0.0f64;
        for &a in legal {
            let idx = a as usize;
            let val = 0.55 * model_avg[idx] + 0.45 * fallback[idx];
            blended[idx] = val.max(0.0);
            sum += blended[idx];
        }
        if sum > 1e-9 {
            for &a in legal {
                blended[a as usize] /= sum;
            }
            blended.to_vec()
        } else {
            fallback.to_vec()
        }
    } else {
        fallback.to_vec()
    }
}

fn pick_preferred_action(legal: &[u8], preferred: &[u8], rng: &mut SmallRng) -> u8 {
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

fn sample_from_probs(probs: &[f64], legal: &[u8], rng: &mut SmallRng) -> u8 {
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
fn sample_opponent_action(
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
                let (idx, _) = lil_poker_mccfr::cfr::abstraction::preflop_bucket(hole[0], hole[1]);
                if idx <= 20 {
                    pick_preferred_action(
                        &legal,
                        &[RAISE_HALF_POT, RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                        rng,
                    )
                } else if idx <= 55 {
                    if opp_to_call == 0 {
                        pick_preferred_action(
                            &legal,
                            &[RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                            rng,
                        )
                    } else if opp_to_call <= 40 {
                        pick_preferred_action(&legal, &[CALL_CHECK, RAISE_MIN], rng)
                    } else {
                        pick_preferred_action(&legal, &[CALL_CHECK, H_FOLD], rng)
                    }
                } else if idx <= 90 {
                    if opp_to_call == 0 {
                        CALL_CHECK
                    } else if opp_to_call <= 20 {
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
                let bucket = lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket(hole, board);
                let (has_fd, has_sd) = lil_poker_mccfr::cfr::abstraction::detect_draws(hole, board);
                if bucket >= 25 {
                    if rng.gen_bool(0.75) {
                        pick_preferred_action(
                            &legal,
                            &[RAISE_HALF_POT, RAISE_THIRD_POT, RAISE_MIN, CALL_CHECK],
                            rng,
                        )
                    } else {
                        CALL_CHECK
                    }
                } else if bucket >= 20 || has_fd || has_sd {
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
                    } else if opp_to_call <= 60 {
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
                    20
                } else {
                    lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket(hole, board)
                };
                if (opp_to_call > 100 && bucket < 16 && rng.gen_bool(0.35))
                    || (opp_to_call > 250 && bucket < 22 && rng.gen_bool(0.60))
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
                let (idx, _) = lil_poker_mccfr::cfr::abstraction::preflop_bucket(hole[0], hole[1]);
                if idx <= 15 {
                    pick_preferred_action(&legal, &[RAISE_MIN, RAISE_THIRD_POT, CALL_CHECK], rng)
                } else if idx <= 30 && opp_to_call <= 20 {
                    CALL_CHECK
                } else {
                    if opp_to_call == 0 {
                        CALL_CHECK
                    } else {
                        H_FOLD
                    }
                }
            } else {
                let bucket = lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket(hole, board);
                if bucket >= 26 {
                    pick_preferred_action(
                        &legal,
                        &[RAISE_HALF_POT, RAISE_THIRD_POT, CALL_CHECK],
                        rng,
                    )
                } else if bucket >= 23 && opp_to_call <= 40 {
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
                let probs = find_strategy_or_fallback(
                    strat,
                    hole,
                    board,
                    game.round,
                    opp_to_call,
                    pot,
                    &legal,
                );
                sample_from_probs(&probs, &legal, rng)
            } else {
                let probs = lil_poker_mccfr::cfr::fallback::get_holdem_fallback_strategy(
                    hole,
                    board,
                    game.round,
                    opp_to_call,
                    pot,
                    &legal,
                );
                sample_from_probs(&probs, &legal, rng)
            }
        }
        _ => legal[rng.gen_range(0..legal.len())],
    }
}

/* Simulate a single hand to completion */
#[allow(clippy::too_many_arguments)]
fn simulate_holdem_hand(
    mut game: TexasHoldemGame,
    my_player: usize,
    strategy: &Option<HoldemStrategy>,
    opp_archetype: &str,
    opp_strategy: &Option<HoldemStrategy>,
    subgame_solver: &SubgameSolver,
    enable_subgame_search: bool,
    opp_tracker: &mut OpponentTracker,
    rng: &mut SmallRng,
    delay_ms: u64,
    show_logs: bool,
    label: &str,
) -> f64 {
    if show_logs {
        println!("\n--- {} ---", label);
    }

    let initial_chips = 1000i32;
    let hole = game.hole[my_player];
    let card_str = format!("[{}, {}]", hole[0], hole[1]);

    let mut step_count = 0;
    let mut cumulative_reward = 0.0f64;
    let mut prev_my_contrib = game.contributions[my_player];

    while !game.is_terminal() {
        let cp = game.current_player();

        if cp == my_player {
            step_count += 1;
            let phase_str = match game.round {
                1 => "Preflop",
                2 => "Flop",
                3 => "Turn",
                4 => "River",
                _ => "Waiting",
            };

            let board_str = format!(
                "[{}]",
                game.board
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let legal = game.legal_actions();
            let opp = 1 - my_player;
            let to_call = (game.contributions[opp] - game.contributions[my_player]).max(0);
            let pot = game.contributions[0] + game.contributions[1];

            /* 1. Get raw Blueprint/Subgame probabilities */
            let raw_probs =
                if enable_subgame_search && (game.round >= 3 || (game.round == 2 && pot >= 120)) {
                    subgame_solver.solve_with_state(
                        &game.hole[my_player],
                        &game.board,
                        game.round,
                        &game.history,
                        my_player,
                        game.contributions,
                        game.raises_this_round,
                    )
                } else if let Some(ref strat) = strategy {
                    let key = get_holdem_infoset_key(
                        &game.hole[my_player],
                        &game.board,
                        game.round,
                        &game.history,
                    );
                    if let Some(s) = strat.get(&key) {
                        s.to_vec()
                    } else {
                        find_strategy_or_fallback(
                            strat,
                            &game.hole[my_player],
                            &game.board,
                            game.round,
                            to_call,
                            pot,
                            &legal,
                        )
                    }
                } else {
                    lil_poker_mccfr::cfr::fallback::get_holdem_fallback_strategy(
                        &game.hole[my_player],
                        &game.board,
                        game.round,
                        to_call,
                        pot,
                        &legal,
                    )
                    .to_vec()
                };

            /* 2. Adjust using Opponent Tracker with hand context */
            let current_bucket = if game.round == 1 {
                lil_poker_mccfr::cfr::abstraction::preflop_bucket(
                    game.hole[my_player][0],
                    game.hole[my_player][1],
                )
                .0
            } else {
                lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket(
                    &game.hole[my_player],
                    &game.board,
                )
            };

            /* 3. Sample action with Purification, Opponent Exploitation, and All-in Defense */
            let act = opp_tracker.select_action_purified(
                &raw_probs,
                &legal,
                current_bucket,
                game.round == 1,
                to_call,
                rng,
            );

            let act_name = match act {
                H_FOLD => "FOLD",
                CALL_CHECK => "CALL_CHECK",
                RAISE_MIN => "RAISE_MIN",
                RAISE_THIRD_POT => "RAISE_THIRD_POT",
                RAISE_HALF_POT => "RAISE_HALF_POT",
                ALL_IN => "ALL_IN",
                _ => "UNKNOWN",
            };

            game = game.apply_action(act);

            let pot = game.contributions[0] + game.contributions[1];
            let my_chips = (initial_chips - game.contributions[my_player]).max(0);

            let delta_contrib = (game.contributions[my_player] - prev_my_contrib) as f64;
            prev_my_contrib = game.contributions[my_player];

            let step_reward = if game.is_terminal() {
                let final_ret = game.get_returns()[my_player];
                final_ret / 1000.0
            } else {
                -0.0005 * delta_contrib
            };
            cumulative_reward += step_reward;

            if show_logs {
                println!(
                    "Step {} | Phase: {} | Cards: {} | Board: {} | Action: {} | Pot: {} | My Chips: {} | Reward: {:+.4}",
                    step_count, phase_str, card_str, board_str, act_name, pot, my_chips, step_reward
                );
            }

            if delay_ms > 0 {
                sleep(Duration::from_millis(delay_ms));
            }
        } else {
            /* Opponent turn */
            let legal = game.legal_actions();
            if legal.is_empty() {
                break;
            }
            let facing_cbet = game.round == 2 && game.raises_this_round > 0;
            let act = sample_opponent_action(opp_archetype, opp_strategy, &game, cp, rng);
            /* Record opponent action for opponent modeling tracker */
            opp_tracker.record_action_street(act, game.round, facing_cbet);
            game = game.apply_action(act);
        }
    }

    opp_tracker.end_hand();
    if !game.board.is_empty() && game.history.iter().all(|rh| !rh.contains(&H_FOLD)) {
        let opp_player = 1 - my_player;
        let opp_raised_river = game.history[3].iter().any(|&a| a >= 2);
        opp_tracker.record_showdown_hand(game.hole[opp_player], &game.board, opp_raised_river);
    }

    let final_ret = game.get_returns()[my_player];

    if show_logs && (step_count == 0 || prev_my_contrib != game.contributions[my_player]) {
        step_count += 1;
        let board_str = format!(
            "[{}]",
            game.board
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let pot = 0;
        let my_chips = (initial_chips + final_ret as i32).max(0);
        let term_reward = final_ret / 1000.0;
        cumulative_reward += term_reward;

        let act_name =
            if step_count == 1 && game.returns[1 - my_player] < 0.0 && game.board.is_empty() {
                "OPP_FOLD"
            } else if prev_my_contrib != game.contributions[my_player] {
                "FOLD"
            } else {
                "SHOWDOWN"
            };

        println!(
            "Step {} | Phase: End | Cards: {} | Board: {} | Action: {} | Pot: {} | My Chips: {} | Reward: {:+.4}",
            step_count, card_str, board_str, act_name, pot, my_chips, term_reward
        );
    }

    if show_logs {
        println!(
            "Finished {} | Total Steps: {} | Cumulative Reward: {:+.4} | Net Chips: {:+}",
            label, step_count, cumulative_reward, final_ret as i32
        );
    }

    final_ret
}

/* Full 52-Card Texas Hold'em Simulation Mode */
#[allow(clippy::too_many_arguments)]
fn run_holdem_episodes_log_mode(
    strategy_path: &str,
    total_episodes: u64,
    delay_ms: u64,
    enable_subgame_search: bool,
    player_arg: &str,
    opp_archetype: &str,
    opp_strategy_path: Option<&str>,
    duplicate_mode: bool,
) {
    let strategy = load_holdem_strategy(strategy_path);
    if strategy.is_some() {
        println!("[holdem] Loaded bot strategy from '{}'", strategy_path);
    } else {
        println!(
            "[holdem] WARNING: strategy file '{}' not found — bot plays using fallback.",
            strategy_path
        );
    }

    let opp_strategy = if let Some(path) = opp_strategy_path {
        println!("[holdem] Loaded opponent strategy from '{}'", path);
        load_holdem_strategy(path)
    } else if opp_archetype.to_lowercase() == "self" {
        strategy.clone()
    } else {
        None
    };

    println!(
        "[holdem] Opponent archetype: '{}'",
        opp_archetype.to_uppercase()
    );
    if duplicate_mode {
        println!("[duplicate] DUPLICATE POKER ACTIVE: Each deal played in reverse seats to eliminate card luck!");
    }
    if enable_subgame_search {
        println!("[subgame] Real-Time Search (Subgame Solving) enabled on Turn & River.");
    }

    let mut rng = SmallRng::from_entropy();
    let mut opp_tracker = OpponentTracker::new();
    let subgame_solver = SubgameSolver::new(2500);

    let mut total_stats = SimStats::default();
    let mut pos_stats = [SimStats::default(), SimStats::default()];
    let mut duplicate_stats = SimStats::default();

    if duplicate_mode {
        let show_logs = delay_ms > 0 || total_episodes <= 10;
        for ep in 1..=total_episodes {
            let mut deck = ALL_52_CARDS;
            for i in 0..9 {
                let j = rng.gen_range(i..52);
                deck.swap(i, j);
            }
            let hole_a = [deck[0], deck[1]];
            let hole_b = [deck[2], deck[3]];
            let runout = &deck[4..9];

            if show_logs {
                println!("\n=== Deal {}/{} (Duplicate) ===", ep, total_episodes);
            }

            // Match 1: Bot is Seat 0 (SB), Opponent is Seat 1 (BB)
            let game1 = TexasHoldemGame::new_dealt(hole_a, hole_b, runout);
            let ret1 = simulate_holdem_hand(
                game1,
                0,
                &strategy,
                opp_archetype,
                &opp_strategy,
                &subgame_solver,
                enable_subgame_search,
                &mut opp_tracker,
                &mut rng,
                delay_ms,
                show_logs,
                &format!("Deal {}/{} - Match 1 (Bot as SB)", ep, total_episodes),
            );
            pos_stats[0].record(ret1);
            total_stats.record(ret1);

            // Match 2: Bot is Seat 1 (BB), Opponent is Seat 0 (SB) with identical deck
            let game2 = TexasHoldemGame::new_dealt(hole_a, hole_b, runout);
            let ret2 = simulate_holdem_hand(
                game2,
                1,
                &strategy,
                opp_archetype,
                &opp_strategy,
                &subgame_solver,
                enable_subgame_search,
                &mut opp_tracker,
                &mut rng,
                delay_ms,
                show_logs,
                &format!("Deal {}/{} - Match 2 (Bot as BB)", ep, total_episodes),
            );
            pos_stats[1].record(ret2);
            total_stats.record(ret2);

            let net_edge = ret1 + ret2;
            duplicate_stats.record(net_edge);

            if show_logs || ep % 100 == 0 || ep == total_episodes {
                println!(
                    "Deal {:>4}/{} | SB: {:>+6.1} | BB: {:>+6.1} | Duplicate Edge: {:>+6.1} chips",
                    ep, total_episodes, ret1, ret2, net_edge
                );
            }
        }
    } else {
        for ep in 1..=total_episodes {
            let my_player = match player_arg {
                "0" | "sb" => 0usize,
                "1" | "bb" => 1usize,
                _ => ((ep - 1) % 2) as usize,
            };
            let game = TexasHoldemGame::new_random(&mut rng);
            let ret = simulate_holdem_hand(
                game,
                my_player,
                &strategy,
                opp_archetype,
                &opp_strategy,
                &subgame_solver,
                enable_subgame_search,
                &mut opp_tracker,
                &mut rng,
                delay_ms,
                true,
                &format!("Episode {}", ep),
            );
            pos_stats[my_player].record(ret);
            total_stats.record(ret);
        }
    }

    if opp_tracker.total_actions() > 0 {
        println!("\n=== Opponent Modeling Tracker Stats ===");
        println!("Opponent Total Hands   : {}", opp_tracker.total_hands);
        println!(
            "Opponent VPIP          : {:.1}%",
            opp_tracker.vpip_hands as f64 / opp_tracker.total_hands.max(1) as f64 * 100.0
        );
        println!(
            "Opponent PFR           : {:.1}%",
            opp_tracker.pfr_hands as f64 / opp_tracker.total_hands.max(1) as f64 * 100.0
        );
        println!(
            "Opponent Action Mix    : Fold: {:.1}% | Call: {:.1}% | Raise: {:.1}%",
            opp_tracker.fold_ratio() * 100.0,
            opp_tracker.call_ratio() * 100.0,
            opp_tracker.raise_ratio() * 100.0
        );
        if opp_tracker.flop_cbet_faced > 0 {
            println!(
                "Flop Fold to C-bet     : {:.1}% ({}/{})",
                (opp_tracker.flop_fold_to_cbet as f64 / opp_tracker.flop_cbet_faced as f64) * 100.0,
                opp_tracker.flop_fold_to_cbet,
                opp_tracker.flop_cbet_faced,
            );
        }
        if opp_tracker.total_hands > 0 {
            println!(
                "Went to Showdown (WTSD): {:.1}% ({}/{})",
                (opp_tracker.went_to_showdown as f64 / opp_tracker.total_hands as f64) * 100.0,
                opp_tracker.went_to_showdown,
                opp_tracker.total_hands,
            );
        }
        let (classified_style, confidence) = opp_tracker.classify_style();
        if classified_style != lil_poker_mccfr::cfr::opponent_model::OpponentStyle::Unknown {
            println!(
                "Inferred Playstyle     : {} (Confidence: {:.1}%)",
                classified_style,
                confidence * 100.0
            );
        }
        if opp_tracker.showdown_hands > 0 {
            println!(
                "Showdown Analytics     : {} showdowns | Bluffs Caught: {} ({:.1}%) | Traps: {} | Loose: {}",
                opp_tracker.showdown_hands,
                opp_tracker.showdown_bluff_count,
                opp_tracker.showdown_bluff_rate() * 100.0,
                opp_tracker.showdown_trap_count,
                opp_tracker.showdown_loose_count,
            );
        }
    }

    let bb_val = 20.0f64;

    if duplicate_mode {
        println!("\n============================================================");
        println!("        ♠️  Texas Hold'em: Duplicate Poker Benchmark  ♥️      ");
        println!("============================================================");
        println!("Opponent Archetype    : {}", opp_archetype.to_uppercase());
        println!("Total Duplicate Deals : {}", duplicate_stats.hands);
        println!("Total Matches Played  : {}", total_stats.hands);
        println!(
            "Bot Net Duplicate Edge: {:+.1} chips",
            duplicate_stats.total_chips
        );
        println!(
            "Duplicate Deal Wins   : Wins: {} ({:.1}%) | Losses: {} ({:.1}%) | Ties: {} ({:.1}%)",
            duplicate_stats.wins,
            duplicate_stats.win_pct(),
            duplicate_stats.losses,
            duplicate_stats.loss_pct(),
            duplicate_stats.ties,
            duplicate_stats.tie_pct(),
        );

        /* In Duplicate Poker, 1 deal = 2 hands played. Skill edge per hand is avg / 2. */
        let edge_per_hand = duplicate_stats.avg_chips() / 2.0;
        let se_per_hand = duplicate_stats.se_chips() / 2.0;
        println!(
            "Skill Edge per Hand   : {:+.3} chips (±{:.3})",
            edge_per_hand, se_per_hand
        );
        println!(
            "Duplicate Win Rate    : {:+.2} bb/100 (±{:.2})",
            (edge_per_hand / bb_val) * 100.0,
            (se_per_hand / bb_val) * 100.0,
        );
        println!(
            "Duplicate Win Rate    : {:+.1} mbb/hand (±{:.1})",
            (edge_per_hand / bb_val) * 1000.0,
            (se_per_hand / bb_val) * 1000.0,
        );
    } else {
        println!("\n============================================================");
        println!("        ♠️  Texas Hold'em: Bot Simulation Summary  ♥️        ");
        println!("============================================================");
        println!("Opponent Archetype  : {}", opp_archetype.to_uppercase());
        println!("Total Hands Played  : {}", total_stats.hands);
        println!(
            "Total Net Profit    : {:+.1} chips",
            total_stats.total_chips
        );
        println!(
            "Results Breakdown   : Wins: {} ({:.1}%) | Losses: {} ({:.1}%) | Ties: {} ({:.1}%)",
            total_stats.wins,
            total_stats.win_pct(),
            total_stats.losses,
            total_stats.loss_pct(),
            total_stats.ties,
            total_stats.tie_pct(),
        );

        let avg = total_stats.avg_chips();
        let se = total_stats.se_chips();
        println!("Average Profit      : {:+.3} chips/hand (±{:.3})", avg, se);
        println!(
            "Win Rate (BB/100)   : {:+.2} bb/100 (±{:.2})",
            (avg / bb_val) * 100.0,
            (se / bb_val) * 100.0,
        );
        println!(
            "Win Rate (mbb/hand) : {:+.1} mbb/hand (±{:.1})",
            (avg / bb_val) * 1000.0,
            (se / bb_val) * 1000.0,
        );
    }

    if pos_stats[0].hands > 0 && pos_stats[1].hands > 0 {
        println!("\n--- Positional Breakdown ---");
        for (p, stat) in pos_stats.iter().enumerate() {
            let pos_name = if p == 0 {
                "Player 0 (SB / Button)"
            } else {
                "Player 1 (BB)"
            };
            let p_avg = stat.avg_chips();
            let p_se = stat.se_chips();
            println!("{}:", pos_name);
            println!("  Hands Played    : {}", stat.hands);
            println!("  Win Percentage  : {:.1}%", stat.win_pct());
            println!("  Net Profit      : {:+.1} chips", stat.total_chips);
            println!("  Avg Profit/Hand : {:+.3} chips (±{:.3})", p_avg, p_se);
            println!(
                "  Win Rate        : {:+.2} bb/100",
                (p_avg / bb_val) * 100.0
            );
        }
    }
    println!("============================================================\n");
}

/* Load Strategy */
fn load_strategy(path: &str) -> Strategy {
    let data = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("Cannot read strategy file: {}", path));
    let json: Value = serde_json::from_str(&data).expect("Invalid JSON format");

    json.as_object()
        .expect("Strategy root must be JSON object")
        .iter()
        .map(|(k, v)| {
            let arr = v.as_array().expect("Expected array");
            let p = |i: usize| arr.get(i).and_then(|x| x.as_f64()).unwrap_or(1.0 / 3.0);
            (k.clone(), [p(0), p(1), p(2)])
        })
        .collect()
}

fn format_card(card: Card) -> String {
    let suit_symbol = match card.suit {
        lil_poker_mccfr::game::card::Suit::Club => "♣",
        lil_poker_mccfr::game::card::Suit::Diamond => "♦",
    };
    let rank_str = match card.rank {
        lil_poker_mccfr::game::card::Rank::Jack => "J",
        lil_poker_mccfr::game::card::Rank::Queen => "Q",
        lil_poker_mccfr::game::card::Rank::King => "K",
    };
    format!("'{}{}'", rank_str, suit_symbol)
}

fn action_rl_name(action: u8, round: u8) -> &'static str {
    match action {
        FOLD => "FOLD",
        CALL => "CALL_CHECK",
        RAISE => {
            if round == 1 {
                "RAISE_MIN"
            } else {
                "RAISE_HALF_POT"
            }
        }
        _ => "UNKNOWN",
    }
}

fn action_name(action: u8) -> &'static str {
    match action {
        FOLD => "FOLD",
        CALL => "CALL / CHECK",
        RAISE => "RAISE",
        _ => "UNKNOWN",
    }
}

/* Mode 3: Leduc RL Episode Step Log Format */
fn run_episodes_log_mode(
    strategy: &Strategy,
    total_episodes: u64,
    delay_ms: u64,
    player_arg: &str,
) {
    let mut rng = SmallRng::from_entropy();

    let mut total_stats = SimStats::default();
    let mut pos_stats = [SimStats::default(), SimStats::default()];

    for ep in 1..=total_episodes {
        println!("\n--- Episode {} ---", ep);

        let mut game = LeducGame::new_random(&mut rng);
        let my_player = match player_arg {
            "0" | "oop" => 0usize,
            "1" | "ip" => 1usize,
            _ => ((ep - 1) % 2) as usize,
        };
        let initial_chips = 1000i32;

        let hole = game.hole[my_player];
        let card_str = format!("[{}]", format_card(hole));

        let mut step_count = 0;
        let mut cumulative_reward = 0.0f64;
        let mut prev_my_contrib = game.contributions[my_player];

        while !game.is_terminal() {
            let cp = game.current_player();

            if cp == my_player {
                step_count += 1;
                let phase_str = match game.round {
                    1 => "Preflop",
                    2 => "Flop",
                    _ => "Waiting",
                };

                let board_str = match game.board {
                    Some(b) => format!("[{}]", format_card(b)),
                    None => "[]".to_string(),
                };

                let probs = get_action_probs(strategy, &game, cp);
                let legal = game.legal_actions();
                let chosen = sample_action(&probs, &mut rng);
                let act = legal[chosen];

                let act_name = action_rl_name(act, game.round);

                game = game.apply_action(act);

                let pot = (game.contributions[0] + game.contributions[1]) * 10;
                let my_chips = initial_chips - (game.contributions[my_player] * 40);

                let delta_contrib = (game.contributions[my_player] - prev_my_contrib) as f64;
                prev_my_contrib = game.contributions[my_player];

                let step_reward = if game.is_terminal() {
                    let final_ret = game.get_returns()[my_player];
                    (final_ret * 40.0) / 1000.0
                } else {
                    -0.0100 * delta_contrib
                };
                cumulative_reward += step_reward;

                println!(
                    "Step {} | Phase: {} | Cards: {} | Board: {} | Action: {} | Pot: {} | My Chips: {} | Reward: {:+.4}",
                    step_count, phase_str, card_str, board_str, act_name, pot, my_chips, step_reward
                );

                if delay_ms > 0 {
                    sleep(Duration::from_millis(delay_ms));
                }
            } else {
                let legal = game.legal_actions();
                let act = legal[rng.gen_range(0..legal.len())];
                game = game.apply_action(act);
            }
        }

        let final_ret = game.get_returns()[my_player];
        total_stats.record(final_ret);
        pos_stats[my_player].record(final_ret);

        if step_count == 0 || prev_my_contrib != game.contributions[my_player] {
            step_count += 1;
            let board_str = match game.board {
                Some(b) => format!("[{}]", format_card(b)),
                None => "[]".to_string(),
            };
            let pot = 0;
            let my_chips = initial_chips + (final_ret * 40.0) as i32;
            let term_reward = (final_ret * 40.0) / 1000.0;
            cumulative_reward += term_reward;

            println!(
                "Step {} | Phase: Waiting | Cards: {} | Board: {} | Action: FOLD | Pot: {} | My Chips: {} | Reward: {:+.4}",
                step_count, card_str, board_str, pot, my_chips, term_reward
            );
        }

        println!(
            "Finished Episode {} | Total Steps: {} | Cumulative Reward: {:+.4} | Net Chips: {:+}",
            ep,
            step_count,
            cumulative_reward,
            (final_ret * 40.0) as i32
        );
    }

    /* Print Leduc Simulation Summary */
    println!("\n============================================================");
    println!("         ♠️  Leduc Poker: Bot Simulation Summary  ♥️         ");
    println!("============================================================");
    println!("Total Hands Played  : {}", total_stats.hands);
    println!(
        "Total Net Profit    : {:+.1} chips",
        total_stats.total_chips
    );
    println!(
        "Results Breakdown   : Wins: {} ({:.1}%) | Losses: {} ({:.1}%) | Ties: {} ({:.1}%)",
        total_stats.wins,
        total_stats.win_pct(),
        total_stats.losses,
        total_stats.loss_pct(),
        total_stats.ties,
        total_stats.tie_pct(),
    );
    let avg = total_stats.avg_chips();
    let se = total_stats.se_chips();
    println!("Average Profit      : {:+.3} chips/hand (±{:.3})", avg, se);
    println!(
        "Win Rate (mbb/hand) : {:+.1} mbb/hand (±{:.1})",
        avg * 1000.0,
        se * 1000.0,
    );

    if pos_stats[0].hands > 0 && pos_stats[1].hands > 0 {
        println!("\n--- Positional Breakdown ---");
        for (p, stat) in pos_stats.iter().enumerate() {
            let pos_name = if p == 0 {
                "Player 0 (OOP)"
            } else {
                "Player 1 (IP)"
            };
            let p_avg = stat.avg_chips();
            let p_se = stat.se_chips();
            println!("{}:", pos_name);
            println!("  Hands Played    : {}", stat.hands);
            println!("  Win Percentage  : {:.1}%", stat.win_pct());
            println!("  Net Profit      : {:+.1} chips", stat.total_chips);
            println!(
                "  Win Rate        : {:+.1} mbb/hand (±{:.1})",
                p_avg * 1000.0,
                p_se * 1000.0
            );
        }
    }
    println!("============================================================\n");
}

/* Mode 1: Spectator Watch Mode */
fn run_watch_mode(strategy: &Strategy, total_hands: u64, delay_ms: u64) {
    let mut rng = SmallRng::from_entropy();
    let mut score = [0.0f64; 2];

    println!(
        "\n>>> Starting Spectator Mode (Bot vs Random) — {} hands <<<\n",
        total_hands
    );

    for hand_num in 1..=total_hands {
        println!("------------------------------------------------------------");
        println!("  🎴 HAND #{}", hand_num);
        println!("------------------------------------------------------------");

        let mut game = LeducGame::new_random(&mut rng);
        let hole0 = game.hole[0];
        let hole1 = game.hole[1];

        println!("Dealt Hole Cards:");
        println!("  🤖 Model  (Player 0): {}", format_card(hole0));
        println!("  🎲 Random (Player 1): {}", format_card(hole1));
        println!(
            "  Pot: {} chips (Antes: P0=1, P1=1)",
            game.contributions[0] + game.contributions[1]
        );

        if delay_ms > 0 {
            sleep(Duration::from_millis(delay_ms));
        }

        let mut prev_round = 1;
        while !game.is_terminal() {
            if game.round != prev_round {
                prev_round = game.round;
                println!("\n--- ROUND 2 (Flop) ---");
                if let Some(board) = game.board {
                    println!("  Community Board Card: {}", format_card(board));
                }
                if delay_ms > 0 {
                    sleep(Duration::from_millis(delay_ms));
                }
            }

            let cp = game.current_player();
            let p_name = if cp == 0 {
                "🤖 Model (P0)"
            } else {
                "🎲 Random (P1)"
            };

            let act = if cp == 0 {
                let probs = get_action_probs(strategy, &game, cp);
                let legal = game.legal_actions();
                let chosen = sample_action(&probs, &mut rng);
                let prob_str: String = legal
                    .iter()
                    .zip(probs.iter())
                    .map(|(&a, &p)| format!("{}={:.2}", action_name(a), p))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!(
                    "  {} evaluates infoset '{}' -> [{}] => Action: {}",
                    p_name,
                    game.infoset_key(cp),
                    prob_str,
                    action_name(legal[chosen])
                );
                legal[chosen]
            } else {
                let legal = game.legal_actions();
                let act = legal[rng.gen_range(0..legal.len())];
                println!(
                    "  {} chooses random action => Action: {}",
                    p_name,
                    action_name(act)
                );
                act
            };

            game = game.apply_action(act);
            println!(
                "  Current Pot: {} chips (P0: {}, P1: {})",
                game.contributions[0] + game.contributions[1],
                game.contributions[0],
                game.contributions[1]
            );

            if delay_ms > 0 {
                sleep(Duration::from_millis(delay_ms));
            }
        }

        let rets = game.get_returns();
        score[0] += rets[0];
        score[1] += rets[1];

        println!("\n🏆 HAND #{} RESULT:", hand_num);
        if let Some(board) = game.board {
            println!("  Board revealed: {}", format_card(board));
            println!(
                "  Showdown hands: P0 {} vs P1 {}",
                format_card(hole0),
                format_card(hole1)
            );
        }
        if rets[0] > 0.0 {
            println!("  --> 🤖 Model (P0) WINS +{:.0} chips!", rets[0]);
        } else if rets[1] > 0.0 {
            println!("  --> 🎲 Random (P1) WINS +{:.0} chips!", rets[1]);
        } else {
            println!("  --> TIE / SPLIT POT!");
        }

        println!(
            "  Cumulative Score: 🤖 Model = {:+.1} chips | 🎲 Random = {:+.1} chips\n",
            score[0], score[1]
        );
        if delay_ms > 0 {
            sleep(Duration::from_millis(delay_ms * 2));
        }
    }

    println!("============================================================");
    println!("FINAL SCORE after {} hands:", total_hands);
    println!(
        "  🤖 Model  (P0): {:+.1} chips ({:+.1} mbb/hand)",
        score[0],
        (score[0] / total_hands as f64) * 1000.0
    );
    println!(
        "  🎲 Random (P1): {:+.1} chips ({:+.1} mbb/hand)",
        score[1],
        (score[1] / total_hands as f64) * 1000.0
    );
    println!("============================================================");
}

/* Mode 2: Interactive Human vs Bot Play */
fn run_human_mode(strategy: &Strategy, total_hands: u64) {
    let mut rng = SmallRng::from_entropy();
    let mut human_score = 0.0f64;
    let mut bot_score = 0.0f64;

    println!("\n>>> Interactive Play: You (Human) vs 🤖 Nash Model <<<");
    println!("You will alternate positions: Hand 1 (P0), Hand 2 (P1), etc.\n");

    for hand_num in 1..=total_hands {
        let human_p = ((hand_num - 1) % 2) as usize;
        let bot_p = 1 - human_p;

        println!("------------------------------------------------------------");
        println!(
            "  🎴 HAND #{}/{}  | You are Player {} ({})",
            hand_num,
            total_hands,
            human_p,
            if human_p == 0 {
                "OOP - First to act"
            } else {
                "IP - In position"
            }
        );
        println!("------------------------------------------------------------");

        let mut game = LeducGame::new_random(&mut rng);
        let human_hole = game.hole[human_p];

        println!("Your Card: {}", format_card(human_hole));
        println!("Pot: 2 chips (Antes: 1 chip each)");

        let mut prev_round = 1;
        while !game.is_terminal() {
            if game.round != prev_round {
                prev_round = game.round;
                println!("\n--- ROUND 2 (Flop) ---");
                if let Some(board) = game.board {
                    println!("  Community Board: {}", format_card(board));
                }
            }

            let cp = game.current_player();

            if cp == human_p {
                let legal = game.legal_actions();
                println!("\nYour turn! Legal actions:");
                for (idx, &act) in legal.iter().enumerate() {
                    println!("  [{}] {}", idx + 1, action_name(act));
                }

                let chosen_act = prompt_action(&legal);
                println!("--> You chose: {}", action_name(chosen_act));
                game = game.apply_action(chosen_act);
            } else {
                let probs = get_action_probs(strategy, &game, bot_p);
                let legal = game.legal_actions();
                let idx = sample_action(&probs, &mut rng);
                let bot_act = legal[idx];
                println!("\n🤖 Bot's turn... Bot chooses: {}", action_name(bot_act));
                game = game.apply_action(bot_act);
            }

            println!(
                "Current Pot: {} chips",
                game.contributions[0] + game.contributions[1]
            );
        }

        let rets = game.get_returns();
        let h_ret = rets[human_p];
        let b_ret = rets[bot_p];
        human_score += h_ret;
        bot_score += b_ret;

        println!("\n🏆 HAND #{}/{} RESULT:", hand_num, total_hands);
        println!("  Bot's Hole Card: {}", format_card(game.hole[bot_p]));
        if let Some(board) = game.board {
            println!("  Board: {}", format_card(board));
        }

        if h_ret > 0.0 {
            println!("  --> 🎉 YOU WIN +{:.0} chips!", h_ret);
        } else if b_ret > 0.0 {
            println!("  --> 🤖 BOT WINS +{:.0} chips!", b_ret);
        } else {
            println!("  --> TIE / SPLIT POT!");
        }

        println!(
            "Total Score: YOU = {:+.1} chips | BOT = {:+.1} chips\n",
            human_score, bot_score
        );
    }

    println!("============================================================");
    println!("MATCH COMPLETE ({}/{} hands):", total_hands, total_hands);
    println!("  YOU : {:+.1} chips", human_score);
    println!("  BOT : {:+.1} chips", bot_score);
    if human_score > bot_score {
        println!("  🎉 CONGRATULATIONS! You beat the Nash Bot!");
    } else if bot_score > human_score {
        println!("  🤖 BOT WINS! The Nash model exploited your play.");
    } else {
        println!("  🤝 EVEN MATCH! Draw.");
    }
    println!("============================================================");
}

fn prompt_action(legal: &[u8]) -> u8 {
    loop {
        print!("Select action (1-{}): ", legal.len());
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            if let Ok(choice) = input.trim().parse::<usize>() {
                if choice >= 1 && choice <= legal.len() {
                    return legal[choice - 1];
                }
            }
        }
        println!("Invalid choice. Please try again.");
    }
}

/* Action Sampling Helpers */
fn get_action_probs(strategy: &Strategy, game: &LeducGame, player: usize) -> Vec<f64> {
    let legal = game.legal_actions();
    let n = legal.len();
    let key = game.infoset_key(player);

    if let Some(s) = strategy.get(&key) {
        let raw: Vec<f64> = legal.iter().map(|&a| s[a as usize].max(0.0)).collect();
        let sum: f64 = raw.iter().sum();
        if sum > 1e-12 {
            return raw.iter().map(|p| p / sum).collect();
        }
    }
    vec![1.0 / n as f64; n]
}

fn sample_action(probs: &[f64], rng: &mut SmallRng) -> usize {
    let r: f64 = rng.gen();
    let mut cum = 0.0;
    for (i, &p) in probs.iter().enumerate() {
        cum += p;
        if r < cum {
            return i;
        }
    }
    probs.len() - 1
}
