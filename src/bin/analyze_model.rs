/*!
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
use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "analyze_model",
    about = "Analyze MCCFR Strategy Model: infoset counts per street, action distributions, and stats"
)]
struct Args {
    /** Path to strategy file (.bin or .json) */
    #[arg(default_value = "models/holdem_abstract_strategy.bin")]
    strategy: String,

    /** Optional filter/search pattern for infoset keys (e.g. "P:AA", "F:B54") */
    #[arg(short, long)]
    pattern: Option<String>,

    /** Maximum number of matching infosets to display when searching */
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
    unique_buckets: HashSet<String>,
}

impl StreetStats {
    fn add(&mut self, probs: &[f64], bucket_id: Option<String>) {
        self.count += 1;
        if let Some(b) = bucket_id {
            self.unique_buckets.insert(b);
        }
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

#[inline(always)]
fn get_stack_idx(key: &str) -> usize {
    if key.starts_with("S15|") {
        0
    } else if key.starts_with("S30|") {
        1
    } else {
        2
    }
}

fn stack_label(idx: usize) -> &'static str {
    match idx {
        0 => "15 BB (S15|)",
        1 => "30 BB (S30|)",
        _ => "100 BB (base)",
    }
}

#[derive(Default)]
struct StackStats {
    total: usize,
    pure_count: usize,
    by_street: [usize; 4],
}

fn extract_bucket_token(key: &str, street: u8) -> Option<String> {
    let base_key = key
        .strip_prefix("S15|")
        .or_else(|| key.strip_prefix("S30|"))
        .unwrap_or(key);
    let prefix = match street {
        1 => "P:",
        2 => "F:B",
        3 => "T:B",
        4 => "R:B",
        _ => return None,
    };
    if street == 1 {
        if let Some(rest) = base_key.strip_prefix("P:") {
            let end = rest.find('/').unwrap_or(rest.len());
            if end > 0 && !rest[..end].contains('|') {
                return Some(rest[..end].to_string());
            }
        }
        None
    } else if let Some(pos) = base_key.find(prefix) {
        let rest = &base_key[pos + prefix.len()..];
        let end = rest.find('/').unwrap_or(rest.len());
        if end > 0 {
            let tok = &rest[..end];
            let trimmed = tok.trim_end_matches(['d', 'w', 'p', 'm']);
            if !trimmed.is_empty() {
                Some(trimmed.to_string())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    }
}

fn extract_flop_texture(key: &str) -> Option<char> {
    let pos = key.find("F:B")?;
    let rest = &key[pos + 3..];
    let end = rest.find('/').unwrap_or(rest.len());
    let tok = &rest[..end];
    let last = tok.chars().last()?;
    if matches!(last, 'd' | 'w' | 'p' | 'm') {
        Some(last)
    } else {
        None
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

    /* Group and compute statistics by street and stack depth */
    let mut stats = [
        StreetStats::default(),
        StreetStats::default(),
        StreetStats::default(),
        StreetStats::default(),
    ];
    let mut stack_stats = [
        StackStats::default(),
        StackStats::default(),
        StackStats::default(),
    ];
    /* Flop texture counters: [dry 'd', wet 'w', paired 'p', monotone 'm', untextured] */
    let mut flop_tex_counts = [0usize; 5];
    let mut postflop_buckets = HashSet::new();

    let mut matches = Vec::new();

    for (key, probs) in &strategy {
        let street = get_street(key);
        let s_idx = get_stack_idx(key);
        if (1..=4).contains(&street) {
            let bucket_tok = extract_bucket_token(key, street);
            if street >= 2 {
                if let Some(ref b) = bucket_tok {
                    if let Ok(num) = b.parse::<usize>() {
                        postflop_buckets.insert(num);
                    }
                }
            }
            if street == 2 {
                match extract_flop_texture(key) {
                    Some('d') => flop_tex_counts[0] += 1,
                    Some('w') => flop_tex_counts[1] += 1,
                    Some('p') => flop_tex_counts[2] += 1,
                    Some('m') => flop_tex_counts[3] += 1,
                    _ => flop_tex_counts[4] += 1,
                }
            }
            stats[(street - 1) as usize].add(probs, bucket_tok);
            let ss = &mut stack_stats[s_idx];
            ss.total += 1;
            ss.by_street[(street - 1) as usize] += 1;
            if probs.iter().copied().fold(0.0f64, f64::max) >= 0.95 {
                ss.pure_count += 1;
            }
        }

        if let Some(ref pat) = args.pattern {
            if key.contains(pat) {
                matches.push((key, probs));
            }
        }
    }

    let pf_hands = stats[0].unique_buckets.len();
    let pf_bucket_count = postflop_buckets.len();
    println!(
        "Buckets:      {} postflop | {} preflop hands",
        pf_bucket_count, pf_hands
    );
    println!("------------------------------------------------------------\n");

    /* Print Street Distribution Table */
    println!("+---------------+----------+---------------+------------+--------------+");
    println!("| Street        |  Buckets |      Infosets |    Share % | Pure Actions |");
    println!("+---------------+----------+---------------+------------+--------------+");
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
            "| {:<13} | {:>8} | {:>13} | {:>9.2}% | {:>10.1}% |",
            street_name(s),
            st.unique_buckets.len(),
            st.count,
            pct,
            pure_pct
        );
    }
    println!("+---------------+----------+---------------+------------+--------------+");
    println!(
        "| TOTAL         | {:>8} | {:>13} |    100.00% |              |",
        pf_bucket_count, total_infosets
    );
    println!("+---------------+----------+---------------+------------+--------------+\n");

    /* Print Stack Depth Distribution Table */
    println!("Stack Depth Distribution:");
    println!("+---------------+---------+----------+----------+----------+-----------+----------+--------+");
    println!("| Stack Depth   | Preflop |     Flop |     Turn |    River |     Total |  Share % | Pure % |");
    println!("+---------------+---------+----------+----------+----------+-----------+----------+--------+");
    for (idx, ss) in stack_stats.iter().enumerate() {
        if ss.total == 0 {
            continue;
        }
        let share_pct = if total_infosets > 0 {
            (ss.total as f64 * 100.0) / total_infosets as f64
        } else {
            0.0
        };
        let pure_pct = (ss.pure_count as f64 * 100.0) / ss.total as f64;
        println!(
            "| {:<13} | {:>7} | {:>8} | {:>8} | {:>8} | {:>9} | {:>7.2}% | {:>5.1}% |",
            stack_label(idx),
            ss.by_street[0],
            ss.by_street[1],
            ss.by_street[2],
            ss.by_street[3],
            ss.total,
            share_pct,
            pure_pct
        );
    }
    println!("+---------------+---------+----------+----------+----------+-----------+----------+--------+");

    let textured_flop_total: usize = flop_tex_counts[..4].iter().sum();
    if textured_flop_total > 0 {
        println!(
            "Flop Textures:  Dry(d)={} | Wet(w)={} | Paired(p)={} | Monotone(m)={} | Untextured={}\n",
            flop_tex_counts[0],
            flop_tex_counts[1],
            flop_tex_counts[2],
            flop_tex_counts[3],
            flop_tex_counts[4],
        );
    } else {
        println!();
    }

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
