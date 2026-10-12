/* Evaluate a trained CFR/MCCFR strategy profile for Leduc Poker or 52-Card Texas Hold'em.
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
    /* Game type: "leduc" or "holdem" (auto-detected if strategy path contains holdem/leduc) */
    #[arg(short, long)]
    game: Option<String>,

    /* Path to strategy JSON file */
    #[arg(short, long)]
    strategy: Option<String>,

    /* Number of hands/deals for simulation benchmark */
    #[arg(short = 'n', long)]
    hands: Option<u64>,

    /* Opponent archetype for Hold'em: tag, fish, maniac, rock, self, random */
    #[arg(short = 'o', long, default_value = "tag")]
    opponent: String,

    /* Optional second strategy file for A/B testing */
    #[arg(long)]
    opp_strategy: Option<String>,

    /* Run in Duplicate Poker mode (each deal played in reverse seats to eliminate card luck) */
    #[arg(short, long, default_value_t = true)]
    duplicate: bool,

    /* Enable real-time subgame solving for Turn and River */
    #[arg(long)]
    subgame: bool,

    /* Opponent exploitation factor (0.0 = pure GTO, 1.0 = full exploit) */
    #[arg(long, default_value_t = 0.70)]
    exploit_factor: f64,

    /* Show detailed strategy profile table for key infosets */
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
            if std::path::Path::new("models/holdem_strategy.json").exists() {
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
        println!("============================================================\n");

        let _res = lil_poker_mccfr::sim::run_holdem_benchmark(
            &strategy_path,
            hands,
            0,
            args.subgame,
            "0",
            &args.opponent,
            args.opp_strategy.as_deref(),
            args.duplicate,
            args.exploit_factor,
            false,
        );
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
