/*! Evaluate a trained CFR/MCCFR strategy profile for Leduc Poker or 52-Card Texas Hold'em.
 * Features:
 *   - Tabular strategy profile inspection (preflop / postflop decisions)
 *   - Simulation against random opponent (win %, profit chips/hand, mbb/hand)
 *   - Full Best Response Exploitability (NashConv) calculation
 */

use std::str::FromStr;

use clap::Parser;
use lil_poker_mccfr::cfr::opponent_model::GuardSet;
use lil_poker_mccfr::sim::OpponentArchetype;
use lil_poker_mccfr::sim::leduc::{
    compute_exploitability, display_sample_strategy, load_strategy, simulate_vs_random,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvalTarget {
    Archetype(OpponentArchetype),
    Gauntlet,
    Multiway,
    Ablate,
    Elo,
}

impl EvalTarget {
    const fn banner_name(self) -> &'static str {
        match self {
            Self::Archetype(arch) => arch.banner_name(),
            Self::Gauntlet => "GAUNTLET",
            Self::Multiway => "MULTIWAY",
            Self::Ablate => "ABLATE",
            Self::Elo => "ELO",
        }
    }
}

impl FromStr for EvalTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.eq_ignore_ascii_case("gauntlet") || trimmed.eq_ignore_ascii_case("all") {
            Ok(Self::Gauntlet)
        } else if trimmed.eq_ignore_ascii_case("multiway") || trimmed.eq_ignore_ascii_case("6max") {
            Ok(Self::Multiway)
        } else if trimmed.eq_ignore_ascii_case("ablate") || trimmed.eq_ignore_ascii_case("ablation")
        {
            Ok(Self::Ablate)
        } else if trimmed.eq_ignore_ascii_case("elo") {
            Ok(Self::Elo)
        } else {
            OpponentArchetype::from_str(trimmed).map(Self::Archetype)
        }
    }
}

fn parse_eval_target(raw: &str) -> Result<EvalTarget, String> {
    EvalTarget::from_str(raw)
}

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

    /** Opponent target: tag, fish, maniac, rock, self, random, gauntlet, 6max, ablate, elo */
    #[arg(short = 'o', long, default_value = "tag", value_parser = parse_eval_target)]
    opponent: EvalTarget,

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

    /** Directory scanned for `.bin` models when running `--opponent elo` */
    #[arg(long, default_value = "models")]
    elo_dir: String,

    /** Optional comma-separated strategy paths for round-robin Elo (`--opponent elo`) */
    #[arg(long)]
    elo_models: Option<String>,
}

fn resolve_holdem_strategy_path(arg: Option<String>) -> String {
    arg.unwrap_or_else(|| {
        for cand in [
            "models/holdem_abstract_strategy.bin",
            "models/holdem_strategy_distilled.bin",
            "models/holdem_strategy.json",
        ] {
            if std::path::Path::new(cand).exists() {
                return cand.to_string();
            }
        }
        "models/holdem_abstract_strategy.json".to_string()
    })
}

fn parse_disabled_guards(raw: Option<&str>) -> GuardSet {
    let Some(spec) = raw else {
        return GuardSet::EMPTY;
    };
    match GuardSet::from_disable_spec(spec) {
        Ok(set) => set,
        Err(err) => {
            eprintln!("[evaluate] Invalid --disable-guard value: {err}");
            std::process::exit(2);
        }
    }
}

fn print_holdem_banner(args: &Args, strategy_path: &str, hands: u64, disabled_guards: GuardSet) {
    println!("============================================================");
    println!("     ♠️  lil-poker-mccfr: Texas Hold'em Evaluation Harness  ♥️   ");
    println!("============================================================");
    println!("Strategy Model        : {strategy_path}");
    println!("Opponent Archetype    : {}", args.opponent.banner_name());
    if let Some(ref opp_strat) = args.opp_strategy {
        println!("Opponent Strategy     : {opp_strat}");
    }
    println!("Total Duplicate Deals : {hands}");
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
        println!("Deterministic Seed    : {s}");
    }
    if !disabled_guards.is_empty() {
        println!("Disabled Guards       : {disabled_guards:?}");
    }
    println!("============================================================\n");
}

fn run_blind_schedule_mode(
    args: &Args,
    strategy_path: &str,
    hands: u64,
    disabled_guards: GuardSet,
) {
    if args.opponent != EvalTarget::Gauntlet {
        let opp_archetype = match args.opponent {
            EvalTarget::Archetype(a) => a,
            _ => OpponentArchetype::Tag,
        };
        let _ = lil_poker_mccfr::sim::run_holdem_blind_schedule_benchmark(
            &lil_poker_mccfr::sim::BlindScheduleConfig {
                strategy_path,
                total_deals: hands,
                hands_per_level: args.level_hands,
                enable_subgame_search: args.subgame,
                opp_archetype,
                opp_strategy_path: args.opp_strategy.as_deref(),
                exploit_factor: args.exploit_factor,
                seed: args.seed,
                disabled_guards,
                quiet: false,
            },
        );
        return;
    }

    let archetypes = OpponentArchetype::GAUNTLET_ROSTER;
    let mut total_bb100 = 0.0;
    println!(
        "=== Rising-BB Blind Schedule Gauntlet ({} hands/level) ===",
        args.level_hands.max(2)
    );
    for (idx, &arch) in archetypes.iter().enumerate() {
        let s = args
            .seed
            .or(Some(2026))
            .map(|v| v.wrapping_add((idx as u64 + 1) * 10_007));
        let res = lil_poker_mccfr::sim::run_holdem_blind_schedule_benchmark(
            &lil_poker_mccfr::sim::BlindScheduleConfig {
                strategy_path,
                total_deals: hands,
                hands_per_level: args.level_hands,
                enable_subgame_search: args.subgame,
                opp_archetype: arch,
                exploit_factor: args.exploit_factor,
                seed: s,
                disabled_guards,
                quiet: true,
                ..Default::default()
            },
        );
        total_bb100 += res.win_rate_bb100;
        println!(
            "  {:<18} | {:>+8.2} bb/100 | CI [{:>+7.2}, {:>+7.2}] | SB {:>+7.2} | BB {:>+7.2}",
            arch.banner_name(),
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
}

fn run_multiway_mode(args: &Args, strategy_path: &str, hands: u64) {
    if args.stack_sweep {
        let mut rows = Vec::new();
        for &bb in &[15u32, 30, 100] {
            let chips = (bb as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
            let wr = lil_poker_mccfr::sim::run_holdem_multiway_benchmark(
                &lil_poker_mccfr::sim::MultiwayBenchmarkConfig {
                    strategy_path,
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
            println!("  {bb:>3} BB : {wr:+.2} bb/100");
        }
    } else {
        let chips = (args.stack_bb.max(2) as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
        let _ = lil_poker_mccfr::sim::run_holdem_multiway_benchmark(
            &lil_poker_mccfr::sim::MultiwayBenchmarkConfig {
                strategy_path,
                total_deals: hands,
                enable_subgame_search: args.subgame,
                exploit_factor: args.exploit_factor,
                seed: args.seed,
                stack_size: chips,
            },
        );
    }
}

fn run_gauntlet_mode(args: &Args, strategy_path: &str, hands: u64, disabled_guards: GuardSet) {
    if args.stack_sweep {
        let mut rows = Vec::new();
        for &bb in &[15u32, 30, 100] {
            let chips = (bb as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
            let res = lil_poker_mccfr::sim::run_holdem_gauntlet_benchmark(
                &lil_poker_mccfr::sim::GauntletConfig {
                    strategy_path,
                    deals_per_archetype: hands,
                    enable_subgame_search: args.subgame,
                    exploit_factor: args.exploit_factor,
                    base_seed: args.seed.or(Some(1337)),
                    disabled_guards,
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
            println!("  {bb:>3} BB Average : {avg:+.2} bb/100");
        }
    } else {
        let chips = (args.stack_bb.max(2) as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
        let _ = lil_poker_mccfr::sim::run_holdem_gauntlet_benchmark(
            &lil_poker_mccfr::sim::GauntletConfig {
                strategy_path,
                deals_per_archetype: hands,
                enable_subgame_search: args.subgame,
                exploit_factor: args.exploit_factor,
                base_seed: args.seed.or(Some(2026)),
                disabled_guards,
                quiet: false,
                stack_size: chips,
            },
        );
    }
}

fn run_standard_holdem_mode(
    args: &Args,
    strategy_path: &str,
    hands: u64,
    disabled_guards: GuardSet,
) {
    let opp_archetype = match args.opponent {
        EvalTarget::Archetype(arch) => arch,
        _ => OpponentArchetype::Tag,
    };
    if args.stack_sweep {
        let mut rows = Vec::new();
        for &bb in &[15u32, 30, 100] {
            let chips = (bb as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
            println!("--- Stack Sweep: {bb} BB ---");
            let res = lil_poker_mccfr::sim::run_holdem_benchmark(
                &lil_poker_mccfr::sim::HoldemBenchmarkConfig {
                    strategy_path,
                    total_episodes: hands,
                    enable_subgame_search: args.subgame,
                    opp_archetype,
                    opp_strategy_path: args.opp_strategy.as_deref(),
                    duplicate_mode: args.duplicate,
                    exploit_factor: args.exploit_factor,
                    seed: args.seed,
                    disabled_guards,
                    stack_size: chips,
                    ..Default::default()
                },
            );
            rows.push((bb, res));
        }
        println!(
            "\n=== Stack Sweep Summary (vs {}) ===",
            opp_archetype.banner_name()
        );
        for (bb, r) in rows {
            println!(
                "  {:>3} BB : {:>+8.2} bb/100 (95% CI [{:>+7.2}, {:>+7.2}]) | SB {:>+7.2} | BB {:>+7.2}",
                bb,
                r.win_rate_bb100,
                r.ci_95_bb100.0,
                r.ci_95_bb100.1,
                r.sb_win_rate_bb100,
                r.bb_win_rate_bb100
            );
        }
    } else {
        let chips = (args.stack_bb.max(2) as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
        let eff_opponent = if args.opp_strategy.is_some() && opp_archetype == OpponentArchetype::Tag
        {
            OpponentArchetype::SelfPlay
        } else {
            opp_archetype
        };
        let _res = lil_poker_mccfr::sim::run_holdem_benchmark(
            &lil_poker_mccfr::sim::HoldemBenchmarkConfig {
                strategy_path,
                total_episodes: hands,
                enable_subgame_search: args.subgame,
                opp_archetype: eff_opponent,
                opp_strategy_path: args.opp_strategy.as_deref(),
                duplicate_mode: args.duplicate,
                exploit_factor: args.exploit_factor,
                seed: args.seed,
                disabled_guards,
                stack_size: chips,
                ..Default::default()
            },
        );
    }
}

fn discover_elo_models(elo_dir: &str, elo_models: Option<&str>) -> Vec<String> {
    if let Some(raw) = elo_models {
        return raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    let mut paths = Vec::new();
    if let Ok(entries) = std::fs::read_dir(elo_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bin")) {
                paths.push(p.to_string_lossy().to_string());
            }
        }
    }
    paths.sort();
    paths
}

fn run_holdem_evaluation(args: Args) {
    let strategy_path = resolve_holdem_strategy_path(args.strategy.clone());
    let hands = args.hands.unwrap_or(5000);
    let disabled_guards = parse_disabled_guards(args.disable_guard.as_deref());
    print_holdem_banner(&args, &strategy_path, hands, disabled_guards);

    if args.blind_schedule {
        run_blind_schedule_mode(&args, &strategy_path, hands, disabled_guards);
    } else {
        match args.opponent {
            EvalTarget::Elo => {
                let model_paths = discover_elo_models(&args.elo_dir, args.elo_models.as_deref());
                let chips =
                    (args.stack_bb.max(2) as i32) * lil_poker_mccfr::game::config::BIG_BLIND;
                let _ = lil_poker_mccfr::sim::run_holdem_elo_tournament(
                    &lil_poker_mccfr::sim::EloTournamentConfig {
                        model_paths: &model_paths,
                        deals_per_pair: hands,
                        enable_subgame_search: args.subgame,
                        exploit_factor: args.exploit_factor,
                        base_seed: args.seed,
                        stack_size: chips,
                        quiet: false,
                    },
                );
            }
            EvalTarget::Ablate => {
                lil_poker_mccfr::sim::run_holdem_ablation_benchmark(
                    &strategy_path,
                    hands,
                    args.subgame,
                    args.exploit_factor,
                    args.seed,
                );
            }
            EvalTarget::Multiway => {
                run_multiway_mode(&args, &strategy_path, hands);
            }
            EvalTarget::Gauntlet => {
                run_gauntlet_mode(&args, &strategy_path, hands, disabled_guards);
            }
            EvalTarget::Archetype(_) => {
                run_standard_holdem_mode(&args, &strategy_path, hands, disabled_guards);
            }
        }
    }
}

fn run_leduc_evaluation(args: Args) {
    let strategy_path = args
        .strategy
        .unwrap_or_else(|| "models/leduc_strategy.json".to_string());
    let hands = args.hands.unwrap_or(500_000);

    println!("============================================================");
    println!("        lil-poker-mccfr: Strategy & Model Evaluator         ");
    println!("============================================================");
    println!("Strategy File : {strategy_path}");

    let strategy = load_strategy(&strategy_path);
    println!("Loaded Infosets: {}", strategy.len());
    display_sample_strategy(&strategy, args.verbose);

    println!("\n--- Simulation Benchmark vs Random Opponent ({hands} hands) ---");
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
    println!("\nCombined Average Win Rate: {combined_mbb:+.1} mbb/hand");

    println!("\n--- Exact Exploitability Analysis (Best Response) ---");
    let nash_conv = compute_exploitability(&strategy);
    let exploitability = nash_conv / 2.0;
    println!("NashConv       : {nash_conv:.6} chips/hand");
    println!("Exploitability : {exploitability:.6} chips/hand");

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

/* Main Entrypoint */
fn main() {
    let args = Args::parse();
    let is_holdem = match args.game.as_deref() {
        Some("holdem") => true,
        Some("leduc") => false,
        _ => args
            .strategy
            .as_deref()
            .is_some_and(|s| s.contains("holdem")),
    };
    if is_holdem {
        run_holdem_evaluation(args);
    } else {
        run_leduc_evaluation(args);
    }
}
