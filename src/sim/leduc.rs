/* Leduc Poker CLI Runner and Match Simulator */
use std::collections::HashMap;
use std::io::{self, Write};
use std::thread::sleep;
use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde_json::Value;

use super::stats::SimStats;
use crate::game::card::Card;
use crate::game::leduc::{LeducGame, CALL, FOLD, RAISE};

/* Leduc strategy: 3 actions [fold, call, raise] */
pub type LeducStrategy = HashMap<String, [f64; 3]>;

/* Keep backward-compat alias used in Leduc helpers. */
pub type Strategy = LeducStrategy;

/* Load Strategy */
pub fn load_strategy(path: &str) -> Strategy {
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

pub fn format_card(card: Card) -> String {
    let suit_symbol = match card.suit {
        crate::game::card::Suit::Club => "♣",
        crate::game::card::Suit::Diamond => "♦",
    };
    let rank_str = match card.rank {
        crate::game::card::Rank::Jack => "J",
        crate::game::card::Rank::Queen => "Q",
        crate::game::card::Rank::King => "K",
    };
    format!("'{}{}'", rank_str, suit_symbol)
}

pub fn action_rl_name(action: u8, round: u8) -> &'static str {
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

pub fn action_name(action: u8) -> &'static str {
    match action {
        FOLD => "FOLD",
        CALL => "CALL / CHECK",
        RAISE => "RAISE",
        _ => "UNKNOWN",
    }
}

pub fn prompt_action(legal: &[u8]) -> u8 {
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
pub fn get_action_probs(strategy: &Strategy, game: &LeducGame, player: usize) -> Vec<f64> {
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

pub fn sample_action(probs: &[f64], rng: &mut SmallRng) -> usize {
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

/* Mode 3: Leduc RL Episode Step Log Format */
pub fn run_episodes_log_mode(
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
pub fn run_watch_mode(strategy: &Strategy, total_hands: u64, delay_ms: u64) {
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
pub fn run_human_mode(strategy: &Strategy, total_hands: u64) {
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
    println!("============================================================\n");
}
