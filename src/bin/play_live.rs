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

    #[arg(short, long, default_value = "models/holdem_abstract_strategy.json")]
    strategy: String,

    #[arg(long, default_value_t = false)]
    subgame_search: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    run_live_bot(
        &args.url,
        &args.room,
        &args.name,
        &args.strategy,
        args.subgame_search,
    )
    .await
}
