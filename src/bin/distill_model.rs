use clap::Parser;
use lil_poker_mccfr::game::config::{
    HOLDEM_NUM_ACTIONS, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER, ROUND_TURN,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "distill_model",
    about = "Distill & Compress Texas Hold'em MCCFR strategy under 400MB RAM"
)]
struct Args {
    /** Input strategy path (.bin, .bincode, or .json) */
    #[arg(default_value = "models/holdem_abstract_strategy.bin")]
    input: String,

    /** Output strategy path (.bin) */
    #[arg(default_value = "models/holdem_strategy_distilled.bin")]
    output: String,

    /** Prune river nodes (since Real-time Subgame Search solves River dynamically) */
    #[arg(long, default_value_t = false)]
    prune_river: bool,

    /** Maximum turn actions to retain in blueprint (default: 3) */
    #[arg(long, default_value_t = 3)]
    max_turn_actions: usize,

    /** Maximum preflop action sequence length for postflop nodes (default: 4, allows 4-bet pots) */
    #[arg(long, default_value_t = 4)]
    max_pf_len: usize,

    /** Maximum river actions to retain in blueprint (default: 3) */
    #[arg(long, default_value_t = 3)]
    max_river_actions: usize,
}

#[inline(always)]
fn strip_stack_prefix(key: &str) -> &str {
    key.strip_prefix("S15|")
        .or_else(|| key.strip_prefix("S30|"))
        .unwrap_or(key)
}

fn get_street(key: &str) -> u8 {
    let key = strip_stack_prefix(key);
    if key.starts_with("R:B") || key.contains("|R:B") {
        ROUND_RIVER
    } else if key.starts_with("T:B") || key.contains("|T:B") {
        ROUND_TURN
    } else if key.starts_with("F:B") || key.contains("|F:B") {
        ROUND_FLOP
    } else {
        ROUND_PREFLOP
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("=== lil-poker-mccfr Model Distiller ===");
    println!("Input Model:        {}", args.input);
    println!("Output Target:      {}", args.output);
    println!("Prune River Nodes:  {}", args.prune_river);
    println!("Max Turn Actions:   {}", args.max_turn_actions);

    let start = Instant::now();
    let loaded = lil_poker_mccfr::cfr::holdem_mccfr::HoldemMCCFRSolver::load_strategy_from_file(
        &args.input,
    )?;
    let raw: HashMap<String, [f32; HOLDEM_NUM_ACTIONS]> = loaded
        .into_iter()
        .map(|(k, v)| {
            let mut arr = [0.0f32; HOLDEM_NUM_ACTIONS];
            for (i, &val) in v.iter().enumerate().take(HOLDEM_NUM_ACTIONS) {
                arr[i] = val as f32;
            }
            (k, arr)
        })
        .collect();
    let total_in = raw.len();
    println!("Loaded {} infosets in {:.2?}.", total_in, start.elapsed());

    let mut distilled: HashMap<String, [f32; HOLDEM_NUM_ACTIONS]> =
        HashMap::with_capacity(total_in.min(1_700_000));

    let mut pf_count = 0usize;
    let mut flop_count = 0usize;
    let mut turn_count = 0usize;
    let mut river_count = 0usize;

    for (k, v) in raw {
        let street = get_street(&k);
        match street {
            ROUND_PREFLOP => {
                /* 1. 100% Preflop (always retained) */
                distilled.insert(k, v);
                pf_count += 1;
            }
            ROUND_FLOP => {
                /* 2. Flop: require preflop length <= max_pf_len */
                let base_k = strip_stack_prefix(&k);
                if base_k.starts_with("P:")
                    && let Some(end) = base_k.find('|')
                {
                    let pf_len = base_k[2..end].len();
                    if pf_len > args.max_pf_len {
                        continue;
                    }
                }
                distilled.insert(k, v);
                flop_count += 1;
            }
            ROUND_TURN => {
                /* 3. Turn: preflop length <= max_pf_len, turn actions <= max_turn_actions */
                if k.starts_with("P:")
                    && let Some(end) = k.find('|')
                {
                    let pf_len = k[2..end].len();
                    if pf_len > args.max_pf_len {
                        continue;
                    }
                }
                if let Some(pos) = k.find("T:B") {
                    let after = &k[pos..];
                    if let Some(slash) = after.find('/') {
                        let turn_act_len = after[slash + 1..].len();
                        if turn_act_len > args.max_turn_actions {
                            continue;
                        }
                    }
                }
                distilled.insert(k, v);
                turn_count += 1;
            }
            ROUND_RIVER => {
                /* 4. River: completely pruned if prune_river is enabled */
                if args.prune_river {
                    continue;
                }
                if k.starts_with("P:")
                    && let Some(end) = k.find('|')
                {
                    let pf_len = k[2..end].len();
                    if pf_len > args.max_pf_len {
                        continue;
                    }
                }
                if let Some(pos) = k.find("R:B") {
                    let after = &k[pos..];
                    if let Some(slash) = after.find('/') {
                        let river_act_len = after[slash + 1..].len();
                        if river_act_len > args.max_river_actions {
                            continue;
                        }
                    }
                }
                distilled.insert(k, v);
                river_count += 1;
            }
            _ => {}
        }
    }

    let out_file = File::create(&args.output)?;
    let mut writer = BufWriter::with_capacity(32 * 1024 * 1024, out_file);
    bincode::serialize_into(&mut writer, &distilled)?;
    writer.flush()?;
    let meta = std::fs::metadata(&args.output)?;
    let out_file_size = meta.len();
    let est_ram_mb = (distilled.len() * 140) as f64 / (1024.0 * 1024.0);

    println!("\n=== Distillation Summary ===");
    println!("Total Input Infosets : {total_in}");
    println!(
        "Retained Infosets    : {} ({:.1}%)",
        distilled.len(),
        (distilled.len() as f64 / total_in as f64) * 100.0
    );
    println!("  - Preflop          : {pf_count}");
    println!("  - Flop             : {flop_count}");
    println!("  - Turn             : {turn_count}");
    println!(
        "  - River            : {} {}",
        river_count,
        if args.prune_river {
            "(pruned for real-time subgame search)"
        } else {
            ""
        }
    );
    println!(
        "Output File Size     : {:.1} MB",
        out_file_size as f64 / (1024.0 * 1024.0)
    );
    println!("Estimated RAM Footprint: {est_ram_mb:.1} MB (Target: <= 400 MB)");
    println!("Done in {:.2?}.", start.elapsed());

    Ok(())
}
