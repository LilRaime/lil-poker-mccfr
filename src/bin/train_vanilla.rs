use clap::Parser;
use lil_poker_mccfr::cfr::vanilla::VanillaCFRSolver;

#[derive(Parser, Debug)]
#[command(about = "Train exact Vanilla CFR+ for Leduc Poker")]
struct Args {
    #[arg(short, long, default_value_t = 5000)]
    iterations: usize,

    #[arg(short, long, default_value = "models/leduc_vanilla.json")]
    save_path: String,
}

fn main() {
    let args = Args::parse();
    println!("=== lil-poker-mccfr: Exact Vanilla CFR+ Solver ===");
    println!("Iterations: {}", args.iterations);
    println!("Save path:  {}", args.save_path);

    let mut solver = VanillaCFRSolver::new(true); /* CFR+ */
    let start = std::time::Instant::now();
    solver.train(args.iterations);
    let elapsed = start.elapsed();

    let strat = solver.export_strategy();
    println!("Training complete in {:.2?}", elapsed);
    println!("Infosets: {}", strat.len());

    let json = serde_json::to_string_pretty(&strat).unwrap();
    std::fs::write(&args.save_path, json).unwrap();
    println!("Saved strategy to {}", args.save_path);
}
