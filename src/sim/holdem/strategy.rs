/*! Texas Hold'em Strategy Loading and Fallback Resolution */
use std::collections::HashMap;

use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};
use crate::cfr::fallback::{FallbackRequest, get_holdem_fallback_strategy};
use crate::game::config::{HOLDEM_NUM_ACTIONS, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER, ROUND_TURN};
use crate::game::holdem::Card as HCard;

/** Hold'em strategy: 6 actions [`fold`, `call_check`, `raise_min`, `raise_third_pot`, `raise_half_pot`, `all_in`] */
pub type HoldemStrategy = HashMap<String, [f64; HOLDEM_NUM_ACTIONS]>;

/** Hold'em Strategy Loader (supports both legacy 4-action, modern 6-action JSON, and .bin/.bincode formats) */
pub fn load_holdem_strategy(path: &str) -> Option<HoldemStrategy> {
    let prefer_json = std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        || path.contains(".json.")
        || path.contains(".tmp.json");

    if !prefer_json {
        let file_bincode = std::fs::File::open(path).ok()?;
        let mut reader = std::io::BufReader::new(file_bincode);
        if let Ok(raw_f32) =
            bincode::deserialize_from::<_, HashMap<String, [f32; HOLDEM_NUM_ACTIONS]>>(&mut reader)
        {
            return Some(
                raw_f32
                    .into_iter()
                    .map(|(k, v)| {
                        let mut arr = [0.0; HOLDEM_NUM_ACTIONS];
                        for i in 0..HOLDEM_NUM_ACTIONS {
                            arr[i] = v[i] as f64;
                        }
                        (k, arr)
                    })
                    .collect(),
            );
        }
        let file_retry = std::fs::File::open(path).ok()?;
        let reader_retry = std::io::BufReader::new(file_retry);
        if let Ok(raw) = bincode::deserialize_from::<_, HashMap<String, Vec<f64>>>(reader_retry) {
            return Some(
                raw.into_iter()
                    .map(|(k, v)| {
                        let strat = if v.len() >= 6 {
                            [v[0], v[1], v[2], v[3], v[4], v[5]]
                        } else if v.len() == 4 {
                            [v[0], v[1], v[2], 0.0, v[3], 0.0]
                        } else {
                            let mut arr = [0.0; HOLDEM_NUM_ACTIONS];
                            for (i, &val) in v.iter().enumerate().take(HOLDEM_NUM_ACTIONS) {
                                arr[i] = val;
                            }
                            arr
                        };
                        (k, strat)
                    })
                    .collect(),
            );
        }
    }

    let file_json = std::fs::File::open(path).ok()?;
    let reader_json = std::io::BufReader::new(file_json);
    let raw: HashMap<String, Vec<f64>> = serde_json::from_reader(reader_json).ok()?;
    Some(
        raw.into_iter()
            .map(|(k, v)| {
                let strat = if v.len() >= 6 {
                    [v[0], v[1], v[2], v[3], v[4], v[5]]
                } else if v.len() == 4 {
                    [v[0], v[1], v[2], 0.0, v[3], 0.0]
                } else {
                    let mut arr = [0.0; HOLDEM_NUM_ACTIONS];
                    for (i, &val) in v.iter().enumerate().take(HOLDEM_NUM_ACTIONS) {
                        arr[i] = val;
                    }
                    arr
                };
                (k, strat)
            })
            .collect(),
    )
}

pub fn find_strategy_or_fallback(
    strategy: &HoldemStrategy,
    hole: &[HCard; 2],
    board: &[HCard],
    round: u8,
    to_call: i32,
    pot: i32,
    legal: &[u8],
) -> Vec<f64> {
    let fallback = get_holdem_fallback_strategy(&FallbackRequest {
        hole: *hole,
        board,
        round,
        to_call,
        pot,
        legal,
        ..Default::default()
    });

    let prefix = if round == ROUND_PREFLOP {
        let (_, name) = preflop_bucket(hole[0], hole[1]);
        format!("P:{name}/")
    } else {
        let bucket = postflop_equity_bucket(hole, board);
        let r_code = match round {
            ROUND_FLOP => "F",
            ROUND_TURN => "T",
            ROUND_RIVER => "R",
            _ => "X",
        };
        format!("{r_code}:B{bucket:02}")
    };

    let matches: Vec<&[f64; HOLDEM_NUM_ACTIONS]> = strategy
        .iter()
        .filter(|(k, _)| {
            if round == ROUND_PREFLOP {
                k.starts_with(&prefix)
            } else {
                k.contains(&prefix)
            }
        })
        .map(|(_, v)| v)
        .collect();

    if !matches.is_empty() {
        let n = matches.len() as f64;
        let mut model_avg = [0.0f64; HOLDEM_NUM_ACTIONS];
        for s in &matches {
            for i in 0..HOLDEM_NUM_ACTIONS {
                model_avg[i] += s[i] / n;
            }
        }
        let mut blended = [0.0f64; HOLDEM_NUM_ACTIONS];
        let mut sum = 0.0f64;
        for &a in legal {
            let idx = a as usize;
            let val = 0.55 * model_avg[idx] + 0.45 * fallback[idx];
            blended[idx] = val.max(0.0);
            sum += blended[idx];
        }
        if sum > 1e-9 {
            for &a in legal {
                blended[a as usize] /= sum;
            }
            blended.to_vec()
        } else {
            fallback.to_vec()
        }
    } else {
        fallback.to_vec()
    }
}
