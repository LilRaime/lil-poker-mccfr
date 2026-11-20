/*! Evaluate a trained CFR/MCCFR strategy profile for Leduc Poker or 52-Card Texas Hold'em.
 * Features:
 *   - Tabular strategy profile inspection (preflop / postflop decisions)
 *   - Simulation against random opponent (win %, profit chips/hand, mbb/hand)
 *   - Full Best Response Exploitability (NashConv) calculation
 */

use clap::Parser;
use lil_poker_mccfr::sim::leduc::{
    compute_exploitability, display_sample_strategy, load_strategy, simulate_vs_random,
};

/* CLI Arguments */
#[derive(Parser, Debug)]
#[command(about = "Evaluate CFR Strategy for Leduc or 52-Card Texas Hold'em Poker")]
struct Args {
    /** Game type: "leduc" or "holdem" (auto-detected if strategy path contains holdem/leduc) */
    #[arg(short, long)]
    game: Option<String>,

    /** Path to strategy JSON file */
    #[arg(short, long)]
    strategy: Option<String>,

    /** Number of hands/deals for simulation benchmark */
    #[arg(short = 'n', long)]
    hands: Option<u64>,

    /** Opponent archetype for Hold'em: tag, fish, maniac, rock, self, random */
    #[arg(short = 'o', long, default_value = "tag")]
    opponent: String,

    /** Optional second strategy file for A/B testing */
    #[arg(long)]
    opp_strategy: Option<String>,

    /** Run in Duplicate Poker mode (each deal played in reverse seats to eliminate card luck) */
    #[arg(short, long, default_value_t = true, action = clap::ArgAction::Set)]
    duplicate: bool,

    /** Enable real-time subgame solving for Turn and River */
    #[arg(long)]
    subgame: bool,

    /** Opponent exploitation factor (0.0 = pure GTO, 1.0 = full exploit) */
    #[arg(long, default_value_t = 0.70)]
    exploit_factor: f64,

    /** Optional deterministic RNG seed for reproducible benchmarks */
    #[arg(long)]
    seed: Option<u64>,

    /** Optional comma-separated list of guard reason codes to disable (for ablation) */
    #[arg(long)]
    disable_guard: Option<String>,

    /** Effective starting stack depth in Big Blinds (default: 100 BB) */
    #[arg(long, default_value_t = 100)]
    stack_bb: u32,

    /** Sweep benchmark across 15 BB, 30 BB, and 100 BB stacks */
    #[arg(long)]
    stack_sweep: bool,

    /** Simulate rising-BB tournament structure (BB steps up every --level-hands hands) */
    #[arg(long)]
    blind_schedule: bool,

    /** Hands per blind level when --blind-schedule is active (default: 20) */
    #[arg(long, default_value_t = 20)]
    level_hands: u64,

    /** Show detailed strategy profile table for key infosets */
    #[arg(short, long)]
    verbose: bool,
}

/* Main Entrypoint */
fn main() {
    let args = Args::parse();

    let is_holdem = match args.game.as_deref() {
        Some("holdem") => true,
        Some("leduc") => false,
        _ => {
            if let Some(ref s) = args.strategy {
                s.contains("holdem")
            } else {
                false
            }
        }
    };

    if is_holdem {
        let strategy_path = args.strategy.unwrap_or_else(|| {
            if std::path::Path::new("models/holdem_abstract_strategy.bin").exists() {
                "models/holdem_abstract_strategy.bin".to_string()
            } else if std::path::Path::new("models/holdem_strategy_distilled.bin").exists() {
                "models/holdem_strategy_distilled.bin".to_string()
            } else if std::path::Path::new("models/holdem_strategy.json").exists() {
                "models/holdem_strategy.json".to_string()
            } else {
                "models/holdem_abstract_strategy.json".to_string()
            }
        });
        let hands = args.hands.unwrap_or(5000);

        println!("============================================================");
        println!("     ♠️  lil-poker-mccfr: Texas Hold'em Evaluation Harness  ♥️   ");
        println!("============================================================");
        println!("Strategy Model        : {}", strategy_path);
        println!("Opponent Archetype    : {}", args.opponent.to_uppercase());
        if let Some(ref opp_strat) = args.opp_strategy {
            println!("Opponent Strategy     : {}", opp_strat);
        }
        println!("Total Duplicate Deals : {}", hands);
        println!(
            "Duplicate Mode        : {}",
            if args.duplicate {
                "Active (Reverse Seats)"
            } else {
                "Single Dealing"
            }
        );
        println!(
            "Subgame Search        : {}",
            if args.subgame { "Enabled" } else { "Disabled" }
        );
        println!("Exploitation Factor   : {:.2}", args.exploit_factor);
        if args.blind_schedule {
            println!(
                "Blind Schedule        : Active ({} hands per level)",
                args.level_hands.max(2)
            );
        } else if args.stack_sweep {
            println!("Stack Depth           : Sweep [15 BB, 30 BB, 100 BB]");
        } else {
            println!("Stack Depth           : {} BB", args.stack_bb.max(2));
        }
        if let Some(s) = args.seed {
            println!("Deterministic Seed    : {}", s);
        }
        let disabled_guards: Vec<String> = args
            .disable_guard
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !disabled_guards.is_empty() {
            println!("Disabled Guards       : {:?}", disabled_guards);
        }
        println!("============================================================\n");

        if args.blind_schedule {
            if args.opponent.eq_ignore_ascii_case("gauntlet")
                || args.opponent.eq_ignore_ascii_case("all")
            {
                let archetypes = [
                    "tag",
                    "calling_station",
                    "maniac",
                    "shove_bot",
                    "trapper",
                    "rock",
                    "self",
                ];
                let mut total_bb100 = 0.0;
                println!(
                    "=== Rising-BB Blind Schedule Gauntlet ({} hands/level) ===",
                    args.level_hands.max(2)
                );
                for (idx, arch) in archetypes.iter().enumerate() {
                    let s = args
                        .seed
                        .or(Some(2026))
                        .map(|v| v.wrapping_add((idx as u64 + 1) * 10_007));
                    let res = lil_poker_mccfr::sim::run_holdem_blind_schedule_benchmark(
                        &lil_poker_mccfr::sim::BlindScheduleConfig {
                            strategy_path: &strategy_path,
                            total_deals: hands,
                            hands_per_level: args.level_hands,
                            enable_subgame_search: args.subgame,
                            opp_archetype: arch,
                            exploit_factor: args.exploit_factor,
                            seed: s,
                            disabled_guards: &disabled_guards,
                            quiet: true,
                            ..Default::default()
                        },
                    );
                    total_bb100 += res.win_rate_bb100;
                    println!(
                        "  {:<18} | {:>+8.2} bb/100 | CI [{:>+7.2}, {:>+7.2}] | SB {:>+7.2} | BB {:>+7.2}",
                        arch.to_uppercase(),
                        res.win_rate_bb100,
                        res.ci_95_bb100.0,
                        res.ci_95_bb100.1,
                        res.sb_win_rate_bb100,
                        res.bb_win_rate_bb100
                    );
                }
                println!(
                    "  GAUNTLET AVERAGE   : {:>+8.2} bb/100",
                    total_bb100 / archetypes.len() as f64
                );
            } else {
                let _ = lil_poker_mccfr::sim::run_holdem_blind_schedule_benchmark(
                    &lil_poker_mccfr::sim::BlindScheduleConfig {
                        strategy_path: &strategy_path,
                        total_deals: hands,
                        hands_per_level: args.level_hands,
                        enable_subgame_search: args.subgame,
                        opp_archetype: &args.opponent,
                        opp_strategy_path: args.opp_strategy.as_deref(),
                        exploit_factor: args.exploit_factor,
                        seed: args.seed,
                        disabled_guards: &disabled_guards,
                        quiet: false,
                    },
                );
            }
            return;
        }

        if args.opponent.eq_ignore_ascii_case("ablate")
            || args.opponent.eq_ignore_ascii_case("ablation")
        {
            lil_poker_mccfr::sim::run_holdem_ablation_benchmark(
                &strategy_path,
                hands,
                args.subgame,
                args.exploit_factor,
                args.seed,
            );
            return;
        }

        if args.opponent.eq_ignore_ascii_case("multiway")
            || args.opponent.eq_ignore_ascii_case("6max")
        {
            if args.stack_sweep {
                let mut rows = Vec::new();
                for &bb in &[15u32, 30, 100] {
                    let chips = (bb as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
                    let wr = lil_poker_mccfr::sim::run_holdem_multiway_benchmark(
                        &lil_poker_mccfr::sim::MultiwayBenchmarkConfig {
                            strategy_path: &strategy_path,
                            total_deals: hands,
                            enable_subgame_search: args.subgame,
                            exploit_factor: args.exploit_factor,
                            seed: args.seed,
                            stack_size: chips,
                        },
                    );
                    rows.push((bb, wr));
                }
                println!("=== 6-Max Stack Sweep Summary ===");
                for (bb, wr) in rows {
                    println!("  {:>3} BB : {:+.2} bb/100", bb, wr);
                }
            } else {
                let chips =
                    (args.stack_bb.max(2) as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
                let _ = lil_poker_mccfr::sim::run_holdem_multiway_benchmark(
                    &lil_poker_mccfr::sim::MultiwayBenchmarkConfig {
                        strategy_path: &strategy_path,
                        total_deals: hands,
                        enable_subgame_search: args.subgame,
                        exploit_factor: args.exploit_factor,
                        seed: args.seed,
                        stack_size: chips,
                    },
                );
            }
            return;
        }

        if args.opponent.eq_ignore_ascii_case("gauntlet")
            || args.opponent.eq_ignore_ascii_case("all")
        {
            if args.stack_sweep {
                let mut rows = Vec::new();
                for &bb in &[15u32, 30, 100] {
                    let chips = (bb as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
                    let res = lil_poker_mccfr::sim::run_holdem_gauntlet_benchmark(
                        &lil_poker_mccfr::sim::GauntletConfig {
                            strategy_path: &strategy_path,
                            deals_per_archetype: hands,
                            enable_subgame_search: args.subgame,
                            exploit_factor: args.exploit_factor,
                            base_seed: args.seed.or(Some(1337)),
                            disabled_guards: &disabled_guards,
                            quiet: false,
                            stack_size: chips,
                        },
                    );
                    let avg = if res.is_empty() {
                        0.0
                    } else {
                        res.iter().map(|(_, r)| r.win_rate_bb100).sum::<f64>() / res.len() as f64
                    };
                    rows.push((bb, avg));
                }
                println!("=== Gauntlet Stack Sweep Summary ===");
                for (bb, avg) in rows {
                    println!("  {:>3} BB Average : {:+.2} bb/100", bb, avg);
                }
            } else {
                let chips =
                    (args.stack_bb.max(2) as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
                let _ = lil_poker_mccfr::sim::run_holdem_gauntlet_benchmark(
                    &lil_poker_mccfr::sim::GauntletConfig {
                        strategy_path: &strategy_path,
                        deals_per_archetype: hands,
                        enable_subgame_search: args.subgame,
                        exploit_factor: args.exploit_factor,
                        base_seed: args.seed.or(Some(2026)),
                        disabled_guards: &disabled_guards,
                        quiet: false,
                        stack_size: chips,
                    },
                );
            }
            return;
        }

        if args.stack_sweep {
            let mut rows = Vec::new();
            for &bb in &[15u32, 30, 100] {
                let chips = (bb as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
                println!("--- Stack Sweep: {} BB ---", bb);
                let res = lil_poker_mccfr::sim::run_holdem_benchmark(
                    &lil_poker_mccfr::sim::HoldemBenchmarkConfig {
                        strategy_path: &strategy_path,
                        total_episodes: hands,
                        enable_subgame_search: args.subgame,
                        opp_archetype: &args.opponent,
                        opp_strategy_path: args.opp_strategy.as_deref(),
                        duplicate_mode: args.duplicate,
                        exploit_factor: args.exploit_factor,
                        seed: args.seed,
                        disabled_guards: &disabled_guards,
                        stack_size: chips,
                        ..Default::default()
                    },
                );
                rows.push((bb, res));
            }
            println!(
                "\n=== Stack Sweep Summary (vs {}) ===",
                args.opponent.to_uppercase()
            );
            for (bb, r) in rows {
                println!(
                    "  {:>3} BB : {:>+8.2} bb/100 (95% CI [{:>+7.2}, {:>+7.2}]) | SB {:>+7.2} | BB {:>+7.2}",
                    bb, r.win_rate_bb100, r.ci_95_bb100.0, r.ci_95_bb100.1, r.sb_win_rate_bb100, r.bb_win_rate_bb100
                );
            }
        } else {
            let chips = (args.stack_bb.max(2) as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
            let eff_opponent = if args.opp_strategy.is_some() && args.opponent == "tag" {
                "model"
            } else {
                args.opponent.as_str()
            };
            let _res = lil_poker_mccfr::sim::run_holdem_benchmark(
                &lil_poker_mccfr::sim::HoldemBenchmarkConfig {
                    strategy_path: &strategy_path,
                    total_episodes: hands,
                    enable_subgame_search: args.subgame,
                    opp_archetype: eff_opponent,
                    opp_strategy_path: args.opp_strategy.as_deref(),
                    duplicate_mode: args.duplicate,
                    exploit_factor: args.exploit_factor,
                    seed: args.seed,
                    disabled_guards: &disabled_guards,
                    stack_size: chips,
                    ..Default::default()
                },
            );
        }
        return;
    }

    let strategy_path = args
        .strategy
        .unwrap_or_else(|| "models/leduc_strategy.json".to_string());
    let hands = args.hands.unwrap_or(500_000);

    println!("============================================================");
    println!("        lil-poker-mccfr: Strategy & Model Evaluator         ");
    println!("============================================================");
    println!("Strategy File : {}", strategy_path);

    let strategy = load_strategy(&strategy_path);
    println!("Loaded Infosets: {}", strategy.len());

    /* 1. Display tabular strategy profile if requested or by default */
    display_sample_strategy(&strategy, args.verbose);

    /* 2. Win rate & Simulation against Random Agent */
    println!(
        "\n--- Simulation Benchmark vs Random Opponent ({} hands) ---",
        hands
    );
    let sim0 = simulate_vs_random(&strategy, hands, 0, 42);
    let sim1 = simulate_vs_random(&strategy, hands, 1, 99);

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
