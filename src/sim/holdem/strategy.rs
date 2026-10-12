/* Texas Hold'em Strategy Loading and Fallback Resolution */
use std::collections::HashMap;

use serde_json::Value;

use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};
use crate::cfr::fallback::get_holdem_fallback_strategy;
use crate::game::config::*;
use crate::game::holdem::Card as HCard;

/* Hold'em strategy: 6 actions [fold, call_check, raise_min, raise_third_pot, raise_half_pot, all_in] */
pub type HoldemStrategy = HashMap<String, [f64; HOLDEM_NUM_ACTIONS]>;

/* Hold'em Strategy Loader (supports both legacy 4-action and modern 6-action formats) */
pub fn load_holdem_strategy(path: &str) -> Option<HoldemStrategy> {
    let data = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&data).ok()?;
    let map = json.as_object()?;
    Some(
        map.iter()
            .filter_map(|(k, v)| {
                let arr = v.as_array()?;
                let p = |i: usize| arr.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0);
                let strat = if arr.len() >= 6 {
                    [p(0), p(1), p(2), p(3), p(4), p(5)]
                } else {
                    /* Map legacy 4-action entries [fold, call, raise_min, raise_half_pot] */
                    [p(0), p(1), p(2), 0.0, p(3), 0.0]
                };
                Some((k.clone(), strat))
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
    let fallback = get_holdem_fallback_strategy(hole, board, round, to_call, pot, legal);

    let prefix = if round == 1 {
        let (_, name) = preflop_bucket(hole[0], hole[1]);
        format!("P:{}/", name)
    } else {
        let bucket = postflop_equity_bucket(hole, board);
        let r_code = match round {
            2 => "F",
            3 => "T",
            4 => "R",
            _ => "X",
        };
        format!("{}:B{:02}", r_code, bucket)
    };

    let matches: Vec<&[f64; 6]> = strategy
        .iter()
        .filter(|(k, _)| {
            if round == 1 {
                k.starts_with(&prefix)
            } else {
                k.contains(&prefix)
            }
        })
        .map(|(_, v)| v)
        .collect();

    if !matches.is_empty() {
        let n = matches.len() as f64;
        let mut model_avg = [0.0f64; 6];
        for s in &matches {
            for i in 0..6 {
                model_avg[i] += s[i] / n;
            }
        }
        let mut blended = [0.0f64; 6];
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
