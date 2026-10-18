use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "models/holdem_strategy.bin".to_string());
    let output_path = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "models/holdem_strategy_distilled.bin".to_string());

    println!(
        "Distilling model from '{}' -> '{}'...",
        input_path, output_path
    );
    let start = Instant::now();

    let file = File::open(&input_path)?;
    let mut reader = BufReader::with_capacity(32 * 1024 * 1024, file);
    let raw: HashMap<String, [f32; HOLDEM_NUM_ACTIONS]> = bincode::deserialize_from(&mut reader)?;
    let total_in = raw.len();
    println!("Loaded {} infosets in {:.2?}.", total_in, start.elapsed());

    let mut distilled: HashMap<String, [f32; HOLDEM_NUM_ACTIONS]> =
        HashMap::with_capacity(1_650_000);

    for (k, v) in raw {
        /* 1. 100% Preflop (Round 1) */
        if !k.starts_with("P:") || (k.contains('/') && !k.contains('|')) {
            distilled.insert(k, v);
            continue;
        }

        /* 2. Postflop: require preflop length <= 3 (single-raised & 3-bet pots) */
        let end = k.find('|').unwrap_or(k.len());
        let pf_len = k[2..end].len();
        if pf_len > 3 {
            continue;
        }

        /* 3. For River: allow up to 2 river actions (bet / raise / call) */
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
    }

    println!(
        "Distilled down to {} infosets ({:.1}% retained).",
        distilled.len(),
        (distilled.len() as f64 / total_in as f64) * 100.0
    );

    let out_file = File::create(&output_path)?;
    let mut writer = BufWriter::with_capacity(32 * 1024 * 1024, out_file);
    bincode::serialize_into(&mut writer, &distilled)?;

    let file_meta = std::fs::metadata(&output_path)?;
    println!(
        "Distilled file size: {:.1} MB",
        file_meta.len() as f64 / (1024.0 * 1024.0)
    );

    Ok(())
}
