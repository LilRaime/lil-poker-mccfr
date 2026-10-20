/*
 * Model Analyzer for Texas Hold'em MCCFR Strategy.
 * Analyzes strategy files (.bin, .json), displaying:
 * - Street distribution (Preflop, Flop, Turn, River)
 * - Average action frequencies per street
 * - Pure vs Mixed strategy breakdown
 * - Memory footprint and infoset lookup
 */

use clap::Parser;
use lil_poker_mccfr::cfr::holdem_mccfr::HoldemMCCFRSolver;
use lil_poker_mccfr::game::config::{
    ALL_IN, CALL_CHECK, FOLD, HOLDEM_NUM_ACTIONS, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use std::fs::File;
use std::path::Path;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "analyze_model",
    about = "Analyze MCCFR Strategy Model: infoset counts per street, action distributions, and stats"
)]
struct Args {
    /* Path to strategy file (.bin or .json) */
    #[arg(default_value = "models/holdem_abstract_strategy.bin")]
    strategy: String,

    /* Optional filter/search pattern for infoset keys (e.g. "P:AA", "F:B54") */
    #[arg(short, long)]
    pattern: Option<String>,

    /* Maximum number of matching infosets to display when searching */
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[inline(always)]
fn get_street(key: &str) -> u8 {
    if key.starts_with("R:B") || key.contains("|R:B") {
        4
    } else if key.starts_with("T:B") || key.contains("|T:B") {
        3
    } else if key.starts_with("F:B") || key.contains("|F:B") {
        2
    } else {
        1
    }
}

fn street_name(street: u8) -> &'static str {
    match street {
        1 => "Preflop",
        2 => "Flop",
        3 => "Turn",
        4 => "River",
        _ => "Unknown",
    }
}

fn action_name(idx: usize) -> &'static str {
    match idx as u8 {
        FOLD => "Fold",
        CALL_CHECK => "Check/Call",
        RAISE_MIN => "Raise Min",
        RAISE_THIRD_POT => "Raise 33%",
        RAISE_HALF_POT => "Raise 50-75%",
        ALL_IN => "All-In",
        _ => "Other",
    }
}

#[derive(Default)]
struct StreetStats {
    count: usize,
    pure_count: usize,
    action_sums: [f64; HOLDEM_NUM_ACTIONS],
}

impl StreetStats {
    fn add(&mut self, probs: &[f64]) {
        self.count += 1;
        let mut max_p = 0.0f64;
        for (i, &p) in probs.iter().enumerate() {
            if i < HOLDEM_NUM_ACTIONS {
                self.action_sums[i] += p;
                if p > max_p {
                    max_p = p;
                }
            }
        }
        /* A pure strategy has >= 95% on a single action */
        if max_p >= 0.95 {
            self.pure_count += 1;
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let path = Path::new(&args.strategy);

    if !path.exists() {
        eprintln!("Error: Strategy file '{}' not found!", args.strategy);
        std::process::exit(1);
    }

    let file_size = File::open(path)?.metadata()?.len();

    println!("============================================================");
    println!("        LIL-POKER MCCFR MODEL ANALYZER & INSPECTOR          ");
    println!("============================================================");
    println!("File:         {}", args.strategy);
    println!("File Size:    {}", format_bytes(file_size));

    let start = Instant::now();
    let strategy = HoldemMCCFRSolver::load_strategy_from_file(&args.strategy)?;
    let load_time = start.elapsed();
    let total_infosets = strategy.len();

    println!("Load Time:    {:.2?}", load_time);
    println!("Total Nodes:  {} infosets", total_infosets);
    /* Approximate memory in RAM: key string (~32 bytes) + Vec capacity (~32 bytes) + HashMap overhead (~32 bytes) */
    let est_ram = (total_infosets * 128) as u64;
    println!("RAM Estimate: {}", format_bytes(est_ram));
    println!("------------------------------------------------------------\n");

    /* Group and compute statistics by street */
    let mut stats = [
        StreetStats::default(),
        StreetStats::default(),
        StreetStats::default(),
        StreetStats::default(),
    ];

    let mut matches = Vec::new();

    for (key, probs) in &strategy {
        let street = get_street(key);
        if (1..=4).contains(&street) {
            stats[(street - 1) as usize].add(probs);
        }

        if let Some(ref pat) = args.pattern {
            if key.contains(pat) {
                matches.push((key, probs));
            }
        }
    }

    /* Print Street Distribution Table */
    println!("+---------------+---------------+------------+--------------+");
    println!("| Street        |      Infosets |    Share % | Pure Actions |");
    println!("+---------------+---------------+------------+--------------+");
    for s in 1..=4 {
        let st = &stats[(s - 1) as usize];
        let pct = if total_infosets > 0 {
            (st.count as f64 * 100.0) / total_infosets as f64
        } else {
            0.0
        };
        let pure_pct = if st.count > 0 {
            (st.pure_count as f64 * 100.0) / st.count as f64
        } else {
            0.0
        };
        println!(
            "| {:<13} | {:>13} | {:>9.2}% | {:>10.1}% |",
            street_name(s),
            st.count,
            pct,
            pure_pct
        );
    }
    println!("+---------------+---------------+------------+--------------+");
    println!(
        "| TOTAL         | {:>13} |    100.00% |              |",
        total_infosets
    );
    println!("+---------------+---------------+------------+--------------+\n");

    /* Print Average Action Probabilities by Street */
    println!("Average Action Distributions by Street:");
    println!("+----------+--------+------------+-----------+-----------+------------+--------+");
    println!("| Street   |   Fold | Check/Call | Raise Min | Raise 33% | Raise 50%+ | All-In |");
    println!("+----------+--------+------------+-----------+-----------+------------+--------+");
    for s in 1..=4 {
        let st = &stats[(s - 1) as usize];
        let n = st.count.max(1) as f64;
        let f = (st.action_sums[FOLD as usize] / n) * 100.0;
        let c = (st.action_sums[CALL_CHECK as usize] / n) * 100.0;
        let r_min = (st.action_sums[RAISE_MIN as usize] / n) * 100.0;
        let r_3rd = (st.action_sums[RAISE_THIRD_POT as usize] / n) * 100.0;
        let r_half = (st.action_sums[RAISE_HALF_POT as usize] / n) * 100.0;
        let r_allin = (st.action_sums[ALL_IN as usize] / n) * 100.0;
        println!(
            "| {:<8} | {:>5.1}% | {:>9.1}% | {:>8.1}% | {:>8.1}% | {:>9.1}% | {:>5.1}% |",
            street_name(s),
            f,
            c,
            r_min,
            r_3rd,
            r_half,
            r_allin
        );
    }
    println!("+----------+--------+------------+-----------+-----------+------------+--------+\n");

    /* If search pattern was provided, show sample matching keys */
    if let Some(ref pat) = args.pattern {
        println!(
            "Matching Infosets for pattern '{}' (found {}):",
            pat,
            matches.len()
        );
        matches.sort_by(|a, b| a.0.cmp(b.0));
        let shown = matches.len().min(args.limit);
        for &(k, probs) in matches.iter().take(shown) {
            print!("  • {:<32} => [", k);
            for (i, &p) in probs.iter().enumerate() {
                if i > 0 {
                    print!(", ");
                }
                print!("{}: {:.1}%", action_name(i), p * 100.0);
            }
            println!("]");
        }
        if matches.len() > shown {
            println!("  ... and {} more matching keys", matches.len() - shown);
        }
        println!();
    }

    Ok(())
}
