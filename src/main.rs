mod game;
mod cfr;

use clap::Parser;
use std::fs;
use cfr::mccfr::MCCFRSolver;


/* Parallel MCCFR (CFR+) Poker Solver — Leduc Hold'em */
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /* Number of MCCFR iterations */
    #[arg(short, long, default_value_t = 200_000)]
    iterations: u64,

    /* Number of parallel threads (Rayon) */
    #[arg(short, long, default_value_t = num_cpus())]
    threads: usize,

    /* Path to save the strategy JSON */
    #[arg(short, long, default_value = "strategy.json")]
    save_path: String,

    /* Log progress every N iterations (0 = silent) */
    #[arg(long, default_value_t = 10_000)]
    log_every: u64,
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn main() {
    let args = Args::parse();

    println!("=== lil-poker-mccfr: Parallel MCCFR (CFR+) ===");
    println!("Game:       Leduc Hold'em");
    println!("Iterations: {}", args.iterations);
    println!("Threads:    {}", args.threads);
    println!("Save path:  {}", args.save_path);
    println!();

    let solver = MCCFRSolver::new();

    let start = std::time::Instant::now();
    solver.train(args.iterations, args.threads, args.log_every);
    let elapsed = start.elapsed();

    let strategy = solver.export_strategy();
    let node_count = strategy.len();

    println!(
        "Training complete! {} nodes visited in {:.2?}",
        node_count,
        elapsed
    );

    /* Export strategy to JSON */
    let json_map: std::collections::HashMap<String, Vec<f64>> = strategy
        .into_iter()
        .map(|(k, v)| {
            let rounded: Vec<f64> = v.iter().map(|x| (x * 10000.0).round() / 10000.0).collect();
            (k, rounded)
        })
        .collect();

    let json = serde_json::to_string_pretty(&json_map).expect("JSON serialization failed");

    if let Some(parent) = std::path::Path::new(&args.save_path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).expect("Failed to create output directory");
        }
    }

    fs::write(&args.save_path, &json).expect("Failed to write strategy file");
    println!("Strategy saved to {} ({} infosets)", args.save_path, node_count);
}
