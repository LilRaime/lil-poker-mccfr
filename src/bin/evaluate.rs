/* Evaluate a trained CFR/MCCFR strategy profile for Leduc Poker.
 * Features:
 *   - Tabular strategy profile inspection (preflop / postflop decisions)
 *   - Simulation against random opponent (win %, profit chips/hand, mbb/hand)
 *   - Full Best Response Exploitability (NashConv) calculation
 */

use clap::Parser;
use lil_poker_mccfr::game::card::ALL_CARDS;
use lil_poker_mccfr::game::leduc::LeducGame;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde_json::Value;
use std::collections::HashMap;

type Strategy = HashMap<String, [f64; 3]>;

/* CLI Arguments */
#[derive(Parser, Debug)]
#[command(about = "Evaluate CFR Strategy for Leduc Poker (Rust implementation of evaluate.py)")]
struct Args {
    /* Path to strategy JSON file */
    #[arg(short, long, default_value = "models/leduc_strategy.json")]
    strategy: String,

    /* Number of hands for simulation benchmark against random agent */
    #[arg(short = 'n', long, default_value_t = 500_000)]
    hands: u64,

    /* Show detailed strategy profile table for key infosets */
    #[arg(short, long)]
    verbose: bool,
}

/* Main Entrypoint */
fn main() {
    let args = Args::parse();

    println!("============================================================");
    println!("        lil-poker-mccfr: Strategy & Model Evaluator         ");
    println!("============================================================");
    println!("Strategy File : {}", args.strategy);

    let strategy = load_strategy(&args.strategy);
    println!("Loaded Infosets: {}", strategy.len());

    /* 1. Display tabular strategy profile if requested or by default */
    display_sample_strategy(&strategy, args.verbose);

    /* 2. Win rate & Simulation against Random Agent */
    println!(
        "\n--- Simulation Benchmark vs Random Opponent ({} hands) ---",
        args.hands
    );
    let sim0 = simulate_vs_random(&strategy, args.hands, 0, 42);
    let sim1 = simulate_vs_random(&strategy, args.hands, 1, 99);

    println!("Player 0 (OOP / Out of Position):");
    println!("  Win Percentage : {:.2}%", sim0.win_pct);
    println!("  Average Profit : {:+.3} chips/hand", sim0.avg_chips);
    println!(
        "  Win Rate       : {:+.1} mbb/hand (±{:.1})",
        sim0.mbb, sim0.mbb_se
    );

    println!("\nPlayer 1 (IP / In Position):");
    println!("  Win Percentage : {:.2}%", sim1.win_pct);
    println!("  Average Profit : {:+.3} chips/hand", sim1.avg_chips);
    println!(
        "  Win Rate       : {:+.1} mbb/hand (±{:.1})",
        sim1.mbb, sim1.mbb_se
    );

    let combined_mbb = (sim0.mbb + sim1.mbb) / 2.0;
    println!("\nCombined Average Win Rate: {:+.1} mbb/hand", combined_mbb);

    /* 3. Exploitability (Best Response) */
    println!("\n--- Exact Exploitability Analysis (Best Response) ---");
    let nash_conv = compute_exploitability(&strategy);
    let exploitability = nash_conv / 2.0;
    println!("NashConv       : {:.6} chips/hand", nash_conv);
    println!("Exploitability : {:.6} chips/hand", exploitability);

    println!("\n------------------------------------------------------------");
    if exploitability < 0.05 {
        println!("Status: ✅ EXCELLENT — Near Nash Equilibrium (< 0.05)");
    } else if exploitability < 0.25 {
        println!("Status: ✅ VERY GOOD  — Low Exploitability (< 0.25)");
    } else if exploitability < 1.50 {
        println!("Status: ⚠️ ACCEPTABLE — Solid Play, Converging (< 1.50)");
    } else {
        println!("Status: ❌ WEAK — Requires More Training Iterations");
    }
    println!("============================================================\n");
}

/* Load Strategy JSON */
fn load_strategy(path: &str) -> Strategy {
    let data = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("Cannot read strategy file: {}", path));
    let json: Value = serde_json::from_str(&data).expect("Invalid JSON format");

    json.as_object()
        .expect("Strategy JSON root must be an object")
        .iter()
        .map(|(k, v)| {
            let arr = v.as_array().expect("Strategy value must be array");
            let p = |i: usize| arr.get(i).and_then(|x| x.as_f64()).unwrap_or(1.0 / 3.0);
            (k.clone(), [p(0), p(1), p(2)])
        })
        .collect()
}

/* Tabular Strategy Display */
fn display_sample_strategy(strategy: &Strategy, verbose: bool) {
    println!("\n--- Strategy Profile Highlights ---");
    println!(
        "{:<14} | {:<8} | {:<8} | {:<8}",
        "Infoset Key", "Fold", "Call/Check", "Raise"
    );
    println!("--------------------------------------------------");

    let sample_keys = if verbose {
        /* Show 20 representative keys */
        vec![
            "Jc/_//",
            "Jc/_/r/",
            "Qc/_//",
            "Qc/_/r/",
            "Kc/_//",
            "Kc/_/r/",
            "Kc/_/c/",
            "Kc/_/cr/",
            "Jc/Jd/cc/",
            "Jc/Jd/cc/r",
            "Jc/Qd/cc/",
            "Jc/Qd/cc/r",
            "Kc/Jd/cc/",
            "Kc/Jd/cc/r",
            "Kc/Kd/cc/",
            "Kc/Kd/cc/r",
            "Qc/Qd/cc/",
            "Qc/Qd/cc/r",
            "Jc/_/crr/",
            "Kc/_/crr/",
        ]
    } else {
        /* Show core 8 preflop & postflop keys */
        vec![
            "Jc/_//",
            "Jc/_/r/",
            "Qc/_//",
            "Qc/_/r/",
            "Kc/_//",
            "Kc/_/r/",
            "Jc/Jd/cc/",
            "Kc/Kd/cc/",
        ]
    };

    for &key in &sample_keys {
        if let Some(strat) = strategy.get(key) {
            println!(
                "{:<14} | {:<8.3} | {:<8.3} | {:<8.3}",
                key, strat[0], strat[1], strat[2]
            );
        }
    }
}

/* Simulation against Random Agent */
struct SimResult {
    win_pct: f64,
    avg_chips: f64,
    mbb: f64,
    mbb_se: f64,
}

fn simulate_vs_random(
    strategy: &Strategy,
    hands: u64,
    model_player: usize,
    seed: u64,
) -> SimResult {
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut total_payoff = 0.0f64;
    let mut sq_payoff = 0.0f64;
    let mut wins = 0u64;

    for _ in 0..hands {
        let mut game = LeducGame::new_random(&mut rng);
        loop {
            if game.is_terminal() {
                break;
            }
            let cp = game.current_player();
            let act = if cp == model_player {
                sample_model_action(strategy, &game, cp, &mut rng)
            } else {
                let legal = game.legal_actions();
                legal[rng.gen_range(0..legal.len())]
            };
            game = game.apply_action(act);
        }

        let payoff = game.get_returns()[model_player];
        if payoff > 0.0 {
            wins += 1;
        }
        total_payoff += payoff;
        sq_payoff += payoff * payoff;
    }

    let avg_chips = total_payoff / hands as f64;
    let var = sq_payoff / hands as f64 - avg_chips * avg_chips;
    let se_chips = (var / hands as f64).sqrt();

    /* Ante = 1 chip = 1 big blind in Leduc unit */
    let mbb = avg_chips * 1000.0;
    let mbb_se = se_chips * 1000.0;
    let win_pct = (wins as f64 / hands as f64) * 100.0;

    SimResult {
        win_pct,
        avg_chips,
        mbb,
        mbb_se,
    }
}

fn sample_model_action(
    strategy: &Strategy,
    game: &LeducGame,
    player: usize,
    rng: &mut SmallRng,
) -> u8 {
    let legal = game.legal_actions();
    let key = game.infoset_key(player);
    let n = legal.len();

    let probs: Vec<f64> = if let Some(s) = strategy.get(&key) {
        let raw: Vec<f64> = legal.iter().map(|&a| s[a as usize].max(0.0)).collect();
        let sum: f64 = raw.iter().sum();
        if sum > 1e-12 {
            raw.iter().map(|p| p / sum).collect()
        } else {
            vec![1.0 / n as f64; n]
        }
    } else {
        vec![1.0 / n as f64; n]
    };

    let r: f64 = rng.gen();
    let mut cum = 0.0;
    for (i, &p) in probs.iter().enumerate() {
        cum += p;
        if r < cum {
            return legal[i];
        }
    }
    legal[n - 1]
}

/* Best Response Tree Traversal (NashConv) */
fn compute_exploitability(strategy: &Strategy) -> f64 {
    let cards = &ALL_CARDS;
    let n = cards.len();
    let mut br0_sum = 0.0f64;
    let mut br1_sum = 0.0f64;

    for i in 0..n {
        for j in 0..n {
            if j == i {
                continue;
            }
            let rem_boards: Vec<_> = (0..n)
                .filter(|&k| k != i && k != j)
                .map(|k| cards[k])
                .collect();

            let dummy = rem_boards[0];
            let g = LeducGame::new_with_cards(cards[i], cards[j], dummy);

            br0_sum += br_recurse_r1(&g, &rem_boards, 0, strategy);
            br1_sum += br_recurse_r1(&g, &rem_boards, 1, strategy);
        }
    }

    (br0_sum / 30.0) + (br1_sum / 30.0)
}

fn br_recurse_r1(
    game: &LeducGame,
    rem_boards: &[lil_poker_mccfr::game::card::Card],
    br_player: usize,
    strategy: &Strategy,
) -> f64 {
    if game.is_terminal() {
        return game.get_returns()[br_player];
    }

    if game.round == 2 {
        let sum: f64 = rem_boards
            .iter()
            .map(|&b| {
                let mut g2 = LeducGame::new_with_cards(game.hole[0], game.hole[1], b);
                for &a in &game.history[0] {
                    g2 = g2.apply_action(a);
                }
                br_recurse_r2(&g2, br_player, strategy)
            })
            .sum();
        return sum / rem_boards.len() as f64;
    }

    let cp = game.current_player();
    let actions = game.legal_actions();

    if cp == br_player {
        actions
            .iter()
            .map(|&a| br_recurse_r1(&game.apply_action(a), rem_boards, br_player, strategy))
            .fold(f64::NEG_INFINITY, f64::max)
    } else {
        let probs = get_action_probs_evaluate(strategy, game, cp);
        actions
            .iter()
            .zip(probs.iter())
            .map(|(&a, &p)| {
                p * br_recurse_r1(&game.apply_action(a), rem_boards, br_player, strategy)
            })
            .sum()
    }
}

fn br_recurse_r2(game: &LeducGame, br_player: usize, strategy: &Strategy) -> f64 {
    if game.is_terminal() {
        return game.get_returns()[br_player];
    }

    let cp = game.current_player();
    let actions = game.legal_actions();

    if cp == br_player {
        actions
            .iter()
            .map(|&a| br_recurse_r2(&game.apply_action(a), br_player, strategy))
            .fold(f64::NEG_INFINITY, f64::max)
    } else {
        let probs = get_action_probs_evaluate(strategy, game, cp);
        actions
            .iter()
            .zip(probs.iter())
            .map(|(&a, &p)| p * br_recurse_r2(&game.apply_action(a), br_player, strategy))
            .sum()
    }
}

fn get_action_probs_evaluate(strategy: &Strategy, game: &LeducGame, cp: usize) -> Vec<f64> {
    let legal = game.legal_actions();
    let key = game.infoset_key(cp);
    let n = legal.len();

    if let Some(s) = strategy.get(&key) {
        let raw: Vec<f64> = legal.iter().map(|&a| s[a as usize].max(0.0)).collect();
        let sum: f64 = raw.iter().sum();
        if sum > 1e-12 {
            return raw.iter().map(|p| p / sum).collect();
        }
    }
    vec![1.0 / n as f64; n]
}
