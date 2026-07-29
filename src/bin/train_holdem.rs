use clap::Parser;
use lil_poker_mccfr::cfr::holdem_mccfr::HoldemMCCFRSolver;

#[derive(Parser, Debug)]
#[command(about = "Train Parallel Card-Abstracted MCCFR for 52-Card Texas Hold'em")]
struct Args {
    /* Number of training iterations (episodes) */
    #[arg(short, long, default_value_t = 10_000_000)]
    iterations: u64,

    /* Number of parallel threads */
    #[arg(short, long, default_value_t = 16)]
    threads: usize,

    /* Log progress every N iterations */
    #[arg(short, long, default_value_t = 50_000)]
    log_every: u64,

    /* Output file path for strategy JSON */
    #[arg(short, long, default_value = "models/holdem_abstract_strategy.json")]
    save_path: String,
}

fn main() {
    let args = Args::parse();

    println!("=== lil-poker-mccfr: Parallel Card-Abstracted Hold'em MCCFR (CFR+) ===");
    println!("Game:       52-Card Texas Hold'em");
    println!("Iterations: {}", args.iterations);
    println!("Threads:    {}", args.threads);
    println!("Save path:  {}", args.save_path);

    let solver = HoldemMCCFRSolver::new();
    let start = std::time::Instant::now();

    solver.train(args.iterations, args.threads, args.log_every);

    let elapsed = start.elapsed();
    let strategy = solver.export_strategy();

    println!(
        "Training complete! {} abstracted infosets visited in {:.2?}",
        strategy.len(),
        elapsed
    );

    if let Some(parent) = std::path::Path::new(&args.save_path).parent() {
        std::fs::create_dir_all(parent).unwrap_or(());
    }

    let json = serde_json::to_string_pretty(&strategy).expect("Failed to serialize strategy");
    std::fs::write(&args.save_path, json).expect("Failed to save strategy JSON");
    println!("Strategy saved to {} ({} infosets)", args.save_path, strategy.len());
}
