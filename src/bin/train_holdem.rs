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

    /* Output file path for strategy */
    #[arg(short, long, default_value = "models/holdem_abstract_strategy.json")]
    save_path: String,

    /* Save checkpoint every N iterations */
    #[arg(long, default_value_t = 0)]
    checkpoint_every: u64,

    /* Also keep versioned checkpoint files */
    #[arg(long, default_value_t = true)]
    keep_checkpoints: bool,

    /* Retain previous-round betting context (distinguish 3-bet from limp pots) */
    #[arg(long, default_value_t = false)]
    rich_history: bool,

    /* Regret-based pruning (speeds up training 2-3x by skipping sub-optimal branches) */
    #[arg(long, default_value_t = true)]
    pruning: bool,

    /* Discounted CFR (DCFR, Brown & Sandholm 2019, used in Pluribus) */
    #[arg(long, default_value_t = true)]
    dcfr: bool,
}

fn get_checkpoint_path(save_path: &str, done: u64) -> String {
    let path = std::path::Path::new(save_path);
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("strategy");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bin");

    let suffix = if done >= 1_000_000 {
        format!("{}m", done / 1_000_000)
    } else {
        format!("{}k", done / 1_000)
    };

    let filename = format!("{}_{}.{}", stem, suffix, ext);
    if parent.as_os_str().is_empty() {
        filename
    } else {
        parent.join(filename).to_string_lossy().to_string()
    }
}

fn save_atomic(
    solver: &HoldemMCCFRSolver,
    target_path: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let tmp_path = format!(
        "{}.{}.tmp.bin",
        target_path.strip_suffix(".bin").unwrap_or(target_path),
        std::process::id()
    );
    let count = solver.save_strategy_to_file(&tmp_path)?;
    std::fs::rename(&tmp_path, target_path)?;
    Ok(count)
}

fn main() {
    let args = Args::parse();

    println!("=== lil-poker-mccfr: Parallel Card-Abstracted Hold'em MCCFR ===");
    println!("Game:             52-Card Texas Hold'em");
    println!("Iterations:       {}", args.iterations);
    println!("Threads:          {}", args.threads);
    println!("Rich history:     {}", args.rich_history);
    println!("Pruning:          {}", args.pruning);
    println!("DCFR:             {}", args.dcfr);
    println!("Save path:        {}", args.save_path);
    if args.checkpoint_every > 0 {
        println!(
            "Checkpoint every: {} iters (keep_files={})",
            args.checkpoint_every, args.keep_checkpoints
        );
    } else {
        println!("Checkpoint every: disabled");
    }

    let capacity = if args.iterations >= 100_000_000 {
        14_000_000
    } else {
        1_000_000
    };
    let shards = if args.threads >= 8 { 16384 } else { 2048 };
    let solver = HoldemMCCFRSolver::with_options(
        args.rich_history,
        args.pruning,
        args.dcfr,
        capacity,
        shards,
    );
    let start = std::time::Instant::now();

    let save_path = args.save_path.clone();
    let keep_checkpoints = args.keep_checkpoints;

    solver.train_with_checkpoint(
        args.iterations,
        args.threads,
        args.log_every,
        args.checkpoint_every,
        |done, s| {
            let ckpt_start = std::time::Instant::now();
            let ckpt_path = if keep_checkpoints {
                get_checkpoint_path(&save_path, done)
            } else {
                save_path.clone()
            };

            match save_atomic(s, &ckpt_path) {
                Ok(count) => {
                    if keep_checkpoints && ckpt_path != save_path {
                        let _ = std::fs::copy(&ckpt_path, &save_path);
                    }
                    let ckpt_secs = ckpt_start.elapsed().as_secs_f64();
                    let done_label = if done >= 1_000_000 {
                        format!("{}M", done / 1_000_000)
                    } else {
                        format!("{}K", done / 1_000)
                    };
                    eprintln!(
                        "  >>> [Checkpoint {:>5}] Saved to {} ({} infosets) in {:.2}s",
                        done_label, ckpt_path, count, ckpt_secs
                    );
                }
                Err(err) => {
                    eprintln!(
                        "  [WARN] Failed to save checkpoint to {}: {}",
                        ckpt_path, err
                    );
                }
            }
        },
    );

    let elapsed = start.elapsed();
    let node_count = save_atomic(&solver, &args.save_path).expect("Failed to save strategy file");

    if args.keep_checkpoints {
        let final_ckpt = get_checkpoint_path(&args.save_path, args.iterations);
        if final_ckpt != args.save_path {
            let _ = std::fs::copy(&args.save_path, &final_ckpt);
        }
    }

    println!(
        "Training complete! {} abstracted infosets visited and saved to {} in {:.2?}",
        node_count, args.save_path, elapsed
    );
}
