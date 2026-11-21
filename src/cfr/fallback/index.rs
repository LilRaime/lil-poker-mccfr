/*!
 * `StrategyLookupIndex`: Pre-indexed bucket -> averaged strategy lookup table.
 */

use super::heuristic::{get_holdem_fallback_strategy, FallbackRequest};
use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};
use crate::game::config::{
    HOLDEM_NUM_ACTIONS, NUM_EQUITY_BUCKETS, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER, ROUND_TURN,
};
use crate::game::holdem::Card;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct StrategyLookupIndex {
    preflop_avg: HashMap<String, [f64; HOLDEM_NUM_ACTIONS]>,
    flop_check_avg: [[f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
    flop_bet_avg: [[f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
    turn_check_avg: [[f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
    turn_bet_avg: [[f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
    river_check_avg: [[f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
    river_bet_avg: [[f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
    flop_check_valid: [bool; NUM_EQUITY_BUCKETS],
    flop_bet_valid: [bool; NUM_EQUITY_BUCKETS],
    turn_check_valid: [bool; NUM_EQUITY_BUCKETS],
    turn_bet_valid: [bool; NUM_EQUITY_BUCKETS],
    river_check_valid: [bool; NUM_EQUITY_BUCKETS],
    river_bet_valid: [bool; NUM_EQUITY_BUCKETS],
}

impl Default for StrategyLookupIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl StrategyLookupIndex {
    pub fn new() -> Self {
        StrategyLookupIndex {
            preflop_avg: HashMap::new(),
            flop_check_avg: [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
            flop_bet_avg: [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
            turn_check_avg: [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
            turn_bet_avg: [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
            river_check_avg: [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
            river_bet_avg: [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
            flop_check_valid: [false; NUM_EQUITY_BUCKETS],
            flop_bet_valid: [false; NUM_EQUITY_BUCKETS],
            turn_check_valid: [false; NUM_EQUITY_BUCKETS],
            turn_bet_valid: [false; NUM_EQUITY_BUCKETS],
            river_check_valid: [false; NUM_EQUITY_BUCKETS],
            river_bet_valid: [false; NUM_EQUITY_BUCKETS],
        }
    }

    pub fn build_from_slice_map<'a, K, V, I>(iter: I) -> Self
    where
        K: AsRef<str> + 'a,
        V: AsRef<[f64]> + 'a,
        I: IntoIterator<Item = (&'a K, &'a V)>,
    {
        let mut preflop_sums: HashMap<String, [f64; HOLDEM_NUM_ACTIONS]> = HashMap::new();
        let mut preflop_counts: HashMap<String, usize> = HashMap::new();
        let mut flop_acc = StreetBucketAccum::new();
        let mut turn_acc = StreetBucketAccum::new();
        let mut river_acc = StreetBucketAccum::new();

        for (k_ref, v_ref) in iter {
            let key = k_ref.as_ref();
            let slice = v_ref.as_ref();
            if slice.is_empty() {
                continue;
            }
            let mut vals = [0.0f64; HOLDEM_NUM_ACTIONS];
            for (i, &val) in slice.iter().enumerate().take(HOLDEM_NUM_ACTIONS) {
                vals[i] = val;
            }
            let acts = key.split('/').nth(1).unwrap_or("");
            let facing_bet = acts.contains('r')
                || acts.contains('t')
                || acts.contains('h')
                || acts.contains('a');

            if let Some(b) = parse_street_bucket(key, "F:B") {
                flop_acc.add(b, facing_bet, &vals);
            } else if let Some(b) = parse_street_bucket(key, "T:B") {
                turn_acc.add(b, facing_bet, &vals);
            } else if let Some(b) = parse_street_bucket(key, "R:B") {
                river_acc.add(b, facing_bet, &vals);
            } else {
                accumulate_preflop_key(key, &vals, &mut preflop_sums, &mut preflop_counts);
            }
        }

        let preflop_avg = finalize_preflop_avg(preflop_sums, &preflop_counts);
        let (flop_check_avg, flop_check_valid, flop_bet_avg, flop_bet_valid) = flop_acc.finalize();
        let (turn_check_avg, turn_check_valid, turn_bet_avg, turn_bet_valid) = turn_acc.finalize();
        let (river_check_avg, river_check_valid, river_bet_avg, river_bet_valid) =
            river_acc.finalize();

        StrategyLookupIndex {
            preflop_avg,
            flop_check_avg,
            flop_bet_avg,
            turn_check_avg,
            turn_bet_avg,
            river_check_avg,
            river_bet_avg,
            flop_check_valid,
            flop_bet_valid,
            turn_check_valid,
            turn_bet_valid,
            river_check_valid,
            river_bet_valid,
        }
    }

    pub fn build_from_map<K, V>(map: &HashMap<K, V>) -> Self
    where
        K: AsRef<str>,
        V: AsRef<[f64]>,
    {
        Self::build_from_slice_map(map.iter())
    }

    #[inline]
    pub fn query_bucket_avg(
        &self,
        round: u8,
        hole: &[Card; 2],
        board: &[Card],
        to_call: i32,
    ) -> Option<[f64; HOLDEM_NUM_ACTIONS]> {
        if round == ROUND_PREFLOP {
            let (_, name) = preflop_bucket(hole[0], hole[1]);
            self.preflop_avg.get(name).copied()
        } else {
            let bucket = postflop_equity_bucket(hole, board);
            if bucket >= NUM_EQUITY_BUCKETS {
                return None;
            }
            let facing_bet = to_call > 0;
            match round {
                ROUND_FLOP => {
                    if facing_bet {
                        if self.flop_bet_valid[bucket] {
                            Some(self.flop_bet_avg[bucket])
                        } else if self.flop_check_valid[bucket] {
                            Some(self.flop_check_avg[bucket])
                        } else {
                            None
                        }
                    } else if self.flop_check_valid[bucket] {
                        Some(self.flop_check_avg[bucket])
                    } else if self.flop_bet_valid[bucket] {
                        Some(self.flop_bet_avg[bucket])
                    } else {
                        None
                    }
                }
                ROUND_TURN => {
                    if facing_bet {
                        if self.turn_bet_valid[bucket] {
                            Some(self.turn_bet_avg[bucket])
                        } else if self.turn_check_valid[bucket] {
                            Some(self.turn_check_avg[bucket])
                        } else {
                            None
                        }
                    } else if self.turn_check_valid[bucket] {
                        Some(self.turn_check_avg[bucket])
                    } else if self.turn_bet_valid[bucket] {
                        Some(self.turn_bet_avg[bucket])
                    } else {
                        None
                    }
                }
                ROUND_RIVER => {
                    if facing_bet {
                        if self.river_bet_valid[bucket] {
                            Some(self.river_bet_avg[bucket])
                        } else if self.river_check_valid[bucket] {
                            Some(self.river_check_avg[bucket])
                        } else {
                            None
                        }
                    } else if self.river_check_valid[bucket] {
                        Some(self.river_check_avg[bucket])
                    } else if self.river_bet_valid[bucket] {
                        Some(self.river_bet_avg[bucket])
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
    }

    pub fn get_blended_fallback(&self, req: &FallbackRequest<'_>) -> Vec<f64> {
        let fallback = get_holdem_fallback_strategy(req);
        if let Some(avg) = self.query_bucket_avg(req.round, &req.hole, req.board, req.to_call) {
            let mut blended = vec![0.0f64; HOLDEM_NUM_ACTIONS];
            let mut sum = 0.0f64;
            for &a in req.legal {
                let idx = a as usize;
                if idx < HOLDEM_NUM_ACTIONS {
                    let val = 0.60 * avg[idx] + 0.40 * fallback[idx];
                    blended[idx] = val.max(0.0);
                    sum += blended[idx];
                }
            }
            if sum > 1e-9 {
                let inv = 1.0 / sum;
                for &a in req.legal {
                    let idx = a as usize;
                    if idx < HOLDEM_NUM_ACTIONS {
                        blended[idx] *= inv;
                    }
                }
                return blended;
            }
        }
        fallback.to_vec()
    }
}

struct StreetBucketAccum {
    check_sums: [[f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
    check_counts: [usize; NUM_EQUITY_BUCKETS],
    bet_sums: [[f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
    bet_counts: [usize; NUM_EQUITY_BUCKETS],
}

type FinalizedStreetBuckets = (
    [[f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
    [bool; NUM_EQUITY_BUCKETS],
    [[f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
    [bool; NUM_EQUITY_BUCKETS],
);

impl StreetBucketAccum {
    fn new() -> Self {
        Self {
            check_sums: [[0.0f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
            check_counts: [0usize; NUM_EQUITY_BUCKETS],
            bet_sums: [[0.0f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS],
            bet_counts: [0usize; NUM_EQUITY_BUCKETS],
        }
    }

    fn add(&mut self, bucket: usize, facing_bet: bool, vals: &[f64; HOLDEM_NUM_ACTIONS]) {
        if facing_bet {
            for (dst, &v) in self.bet_sums[bucket].iter_mut().zip(vals.iter()) {
                *dst += v;
            }
            self.bet_counts[bucket] += 1;
        } else {
            for (dst, &v) in self.check_sums[bucket].iter_mut().zip(vals.iter()) {
                *dst += v;
            }
            self.check_counts[bucket] += 1;
        }
    }

    fn finalize(&self) -> FinalizedStreetBuckets {
        let mut check_avg = [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut check_valid = [false; NUM_EQUITY_BUCKETS];
        let mut bet_avg = [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut bet_valid = [false; NUM_EQUITY_BUCKETS];
        for b in 0..NUM_EQUITY_BUCKETS {
            if self.check_counts[b] > 0 {
                let n = self.check_counts[b] as f64;
                for i in 0..HOLDEM_NUM_ACTIONS {
                    check_avg[b][i] = self.check_sums[b][i] / n;
                }
                check_valid[b] = true;
            }
            if self.bet_counts[b] > 0 {
                let n = self.bet_counts[b] as f64;
                for i in 0..HOLDEM_NUM_ACTIONS {
                    bet_avg[b][i] = self.bet_sums[b][i] / n;
                }
                bet_valid[b] = true;
            }
        }
        (check_avg, check_valid, bet_avg, bet_valid)
    }
}

fn parse_street_bucket(key: &str, tag: &str) -> Option<usize> {
    let pos = key.find(tag)?;
    let start = pos + tag.len();
    let tok = key.get(start..start + 2)?;
    let b = tok.parse::<usize>().ok()?;
    (b < NUM_EQUITY_BUCKETS).then_some(b)
}

fn accumulate_preflop_key(
    key: &str,
    vals: &[f64; HOLDEM_NUM_ACTIONS],
    preflop_sums: &mut HashMap<String, [f64; HOLDEM_NUM_ACTIONS]>,
    preflop_counts: &mut HashMap<String, usize>,
) {
    let base_key = key
        .strip_prefix("S15|")
        .or_else(|| key.strip_prefix("S30|"))
        .unwrap_or(key);
    if !base_key.starts_with("P:") || base_key.contains('|') {
        return;
    }
    let Some(slash_idx) = base_key.find('/') else {
        return;
    };
    let hist_part = &base_key[slash_idx + 1..];
    let num_raises = hist_part.chars().filter(|&c| c != 'c').count();
    if num_raises > 1 {
        return;
    }
    let hand_name = &base_key[2..slash_idx];
    let sum_entry = preflop_sums
        .entry(hand_name.to_string())
        .or_insert([0.0; HOLDEM_NUM_ACTIONS]);
    for i in 0..HOLDEM_NUM_ACTIONS {
        sum_entry[i] += vals[i];
    }
    *preflop_counts.entry(hand_name.to_string()).or_insert(0) += 1;
}

fn finalize_preflop_avg(
    preflop_sums: HashMap<String, [f64; HOLDEM_NUM_ACTIONS]>,
    preflop_counts: &HashMap<String, usize>,
) -> HashMap<String, [f64; HOLDEM_NUM_ACTIONS]> {
    let mut preflop_avg = HashMap::with_capacity(preflop_sums.len());
    for (name, sum_arr) in preflop_sums {
        let count = preflop_counts.get(&name).copied().unwrap_or(1).max(1) as f64;
        let mut avg_arr = [0.0; HOLDEM_NUM_ACTIONS];
        for i in 0..HOLDEM_NUM_ACTIONS {
            avg_arr[i] = sum_arr[i] / count;
        }
        preflop_avg.insert(name, avg_arr);
    }
    preflop_avg
}
