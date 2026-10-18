use clap::Parser;
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "distill_model",
    about = "Distill & Compress Texas Hold'em MCCFR strategy under 400MB RAM"
)]
struct Args {
    /* Input strategy path (.bin, .bincode, or .json) */
    #[arg(default_value = "models/holdem_abstract_strategy.bin")]
    input: String,

    /* Output strategy path (.bin) */
    #[arg(default_value = "models/holdem_strategy_distilled.bin")]
    output: String,

    /* Prune river nodes (since Real-time Subgame Search solves River dynamically) */
    #[arg(long, default_value_t = false)]
    prune_river: bool,

    /* Maximum turn actions to retain in blueprint (default: 3) */
    #[arg(long, default_value_t = 3)]
    max_turn_actions: usize,
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("=== lil-poker-mccfr Model Distiller ===");
    println!("Input Model:        {}", args.input);
    println!("Output Target:      {}", args.output);
    println!("Prune River Nodes:  {}", args.prune_river);
    println!("Max Turn Actions:   {}", args.max_turn_actions);

    let start = Instant::now();
    let file = File::open(&args.input)?;
    let mut reader = BufReader::with_capacity(32 * 1024 * 1024, file);
    let raw: HashMap<String, [f32; HOLDEM_NUM_ACTIONS]> = bincode::deserialize_from(&mut reader)?;
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
            1 => {
                /* 1. 100% Preflop (always retained) */
                distilled.insert(k, v);
                pf_count += 1;
            }
            2 => {
                /* 2. Flop: require preflop length <= 3 (single-raised & 3-bet pots) */
                if k.starts_with("P:") {
                    if let Some(end) = k.find('|') {
                        let pf_len = k[2..end].len();
                        if pf_len > 3 {
                            continue;
                        }
                    }
                }
                distilled.insert(k, v);
                flop_count += 1;
            }
            3 => {
                /* 3. Turn: preflop length <= 3, turn actions <= max_turn_actions */
                if k.starts_with("P:") {
                    if let Some(end) = k.find('|') {
                        let pf_len = k[2..end].len();
                        if pf_len > 3 {
                            continue;
                        }
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
            4 => {
                /* 4. River: completely pruned if prune_river is enabled */
                if args.prune_river {
                    continue;
                }
                if k.starts_with("P:") {
                    if let Some(end) = k.find('|') {
                        let pf_len = k[2..end].len();
                        if pf_len > 3 {
                            continue;
                        }
                    }
                }
                if let Some(pos) = k.find("R:B") {
                    let after = &k[pos..];
                    if let Some(slash) = after.find('/') {
                        let river_act_len = after[slash + 1..].len();
                        if river_act_len > 2 {
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
    println!("Total Input Infosets : {}", total_in);
    println!(
        "Retained Infosets    : {} ({:.1}%)",
        distilled.len(),
        (distilled.len() as f64 / total_in as f64) * 100.0
    );
    println!("  - Preflop          : {}", pf_count);
    println!("  - Flop             : {}", flop_count);
    println!("  - Turn             : {}", turn_count);
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
    println!(
        "Estimated RAM Footprint: {:.1} MB (Target: <= 400 MB)",
        est_ram_mb
    );
    println!("Done in {:.2?}.", start.elapsed());

    Ok(())
}
