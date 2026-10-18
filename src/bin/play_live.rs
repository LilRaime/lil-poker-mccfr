/*
 * Native Rust Live Client for lil-poker.
 * Connects directly to lil-poker REST API + WebSocket server,
 * loads abstract MCCFR strategy model, and plays in real-time.
 */

use clap::Parser;
use lil_poker_mccfr::live::run_live_bot;

#[derive(Parser, Debug)]
#[command(
    name = "play_live",
    author,
    version,
    about = "Live Native Rust Bot for lil-poker"
)]
struct Args {
    #[arg(short, long, default_value = "http://localhost:8090")]
    url: String,

    #[arg(short, long)]
    room: String,

    #[arg(short, long, default_value = "Rust_CFR_Bot")]
    name: String,

    #[arg(short, long, default_value = "models/holdem_abstract_strategy.bin")]
    strategy: String,

    #[arg(long, default_value_t = true)]
    subgame_search: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let strategy_path = if args.strategy == "models/holdem_abstract_strategy.bin"
        && !std::path::Path::new(&args.strategy).exists()
    {
        if std::path::Path::new("models/holdem_abstract_strategy.json").exists() {
            "models/holdem_abstract_strategy.json".to_string()
        } else if std::path::Path::new("models/holdem_strategy_distilled.bin").exists() {
            "models/holdem_strategy_distilled.bin".to_string()
        } else {
            args.strategy.clone()
        }
    } else {
        args.strategy.clone()
    };

    run_live_bot(
        &args.url,
        &args.room,
        &args.name,
        &strategy_path,
        args.subgame_search,
    )
    .await
}
