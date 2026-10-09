/* Interactive Poker CLI & Game Simulator for Leduc Hold'em and 52-Card Texas Hold'em.
 * Usage:
 *   play --game holdem --mode episodes --hands 7
 *   play --game leduc  --mode episodes --hands 7
 */

use clap::Parser;
use lil_poker_mccfr::sim::holdem::run_holdem_episodes_log_mode;
use lil_poker_mccfr::sim::leduc::{
    load_strategy, run_episodes_log_mode, run_human_mode, run_watch_mode,
};

#[derive(Parser, Debug)]
#[command(about = "Play or Watch Poker CLI Simulation (Leduc or 52-card Texas Hold'em)")]
struct Args {
    /* Strategy JSON file for bot */
    #[arg(short, long, default_value = "models/leduc_strategy.json")]
    strategy: String,

    /* Game variant: 'holdem' (full 52-card) or 'leduc' (6-card) */
    #[arg(short, long, default_value = "holdem")]
    game: String,

    /* Game mode: 'episodes', 'watch', or 'human' */
    #[arg(short, long, default_value = "episodes")]
    mode: String,

    /* Number of hands/episodes to simulate */
    #[arg(short = 'n', long, default_value_t = 7)]
    hands: u64,

    /* Delay in milliseconds between steps in watch mode */
    #[arg(short, long, default_value_t = 0)]
    delay: u64,

    /* Enable Real-Time Subgame Search (Subgame Solving) on Turn/River */
    #[arg(long, default_value_t = true)]
    subgame_search: bool,

    /* Player seat: 'alternate' (default, alternates SB and BB), '0' (SB), or '1' (BB) */
    #[arg(long, default_value = "alternate")]
    player: String,

    /* Opponent archetype: 'random', 'tag', 'calling_station', 'maniac', 'rock', 'self' */
    #[arg(long, alias = "opp-archetype", default_value = "random")]
    opponent: String,

    /* Optional strategy JSON file for opponent (e.g. for model-vs-model benchmarks) */
    #[arg(long)]
    opponent_strategy: Option<String>,

    /* Duplicate Poker mode: each deal is played twice swapping seats to eliminate card luck variance */
    #[arg(long, default_value_t = false)]
    duplicate: bool,
}

fn main() {
    let args = Args::parse();

    if args.game == "holdem" {
        /* Default holdem strategy path if the user didn't override it. */
        let holdem_strategy_path = if args.strategy == "models/leduc_strategy.json" {
            "models/holdem_abstract_strategy.json".to_string()
        } else {
            args.strategy.clone()
        };
        run_holdem_episodes_log_mode(
            &holdem_strategy_path,
            args.hands,
            args.delay,
            args.subgame_search,
            &args.player,
            &args.opponent,
            args.opponent_strategy.as_deref(),
            args.duplicate,
        );
        return;
    }

    let strategy = load_strategy(&args.strategy);

    match args.mode.as_str() {
        "episodes" | "episode" | "rl" | "log" => {
            run_episodes_log_mode(&strategy, args.hands, args.delay, &args.player);
        }
        "watch" | "sim" | "simulation" => {
            println!("============================================================");
            println!("        ♠️  lil-poker-mccfr: Leduc Poker Game CLI  ♥️        ");
            println!("============================================================");
            println!(
                "Loaded Strategy: {} infosets from {}",
                strategy.len(),
                args.strategy
            );
            run_watch_mode(&strategy, args.hands, args.delay);
        }
        _ => {
            println!("============================================================");
            println!("        ♠️  lil-poker-mccfr: Leduc Poker Game CLI  ♥️        ");
            println!("============================================================");
            println!(
                "Loaded Strategy: {} infosets from {}",
                strategy.len(),
                args.strategy
            );
            run_human_mode(&strategy, args.hands);
        }
    }
}
