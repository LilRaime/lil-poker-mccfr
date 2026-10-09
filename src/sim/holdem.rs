/* 52-Card Texas Hold'em Simulation Engine and Match Runner */
use std::collections::HashMap;
use std::thread::sleep;
use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde_json::Value;

use super::opponents::sample_opponent_action;
use super::stats::SimStats;
use crate::cfr::abstraction::{
    get_holdem_infoset_key, get_holdem_infoset_key_rich, postflop_equity_bucket, preflop_bucket,
};
use crate::cfr::fallback::get_holdem_fallback_strategy;
use crate::cfr::opponent_model::{OpponentStyle, OpponentTracker};
use crate::cfr::subgame::SubgameSolver;
use crate::game::holdem::{
    Card as HCard, TexasHoldemGame, ALL_52_CARDS, ALL_IN, CALL_CHECK, FOLD as H_FOLD,
    RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};

/* Hold'em strategy: 6 actions [fold, call_check, raise_min, raise_third_pot, raise_half_pot, all_in] */
pub type HoldemStrategy = HashMap<String, [f64; 6]>;

/* Hold'em Strategy Loader (supports both legacy 4-action and modern 6-action formats) */
pub fn load_holdem_strategy(path: &str) -> Option<HoldemStrategy> {
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

pub fn find_strategy_or_fallback(
    strategy: &HoldemStrategy,
    hole: &[HCard; 2],
    board: &[HCard],
    round: u8,
    to_call: i32,
    pot: i32,
    legal: &[u8],
) -> Vec<f64> {
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
        format!("{}:B{:02}", r_code, bucket)
    };

    let matches: Vec<&[f64; 6]> = strategy
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

/* Simulate a single hand to completion */
#[allow(clippy::too_many_arguments)]
pub fn simulate_holdem_hand(
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
                    let rich_key = get_holdem_infoset_key_rich(
                        &game.hole[my_player],
                        &game.board,
                        game.round,
                        &game.history,
                    );
                    let exact_key = get_holdem_infoset_key(
                        &game.hole[my_player],
                        &game.board,
                        game.round,
                        &game.history,
                    );
                    if let Some(s) = strat.get(&rich_key).or_else(|| strat.get(&exact_key)) {
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
                    get_holdem_fallback_strategy(
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
                preflop_bucket(game.hole[my_player][0], game.hole[my_player][1]).0
            } else {
                postflop_equity_bucket(&game.hole[my_player], &game.board)
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
pub fn run_holdem_episodes_log_mode(
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
        if classified_style != OpponentStyle::Unknown {
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
            (se / bb_val) * 100.0,
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
