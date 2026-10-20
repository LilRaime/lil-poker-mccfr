/*
 * StrategyLookupIndex: Pre-indexed bucket -> averaged strategy lookup table.
 */

use super::heuristic::get_holdem_fallback_strategy;
use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};
use crate::game::config::*;
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

        let mut flop_check_sums = [[0.0f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut flop_check_counts = [0usize; NUM_EQUITY_BUCKETS];
        let mut flop_bet_sums = [[0.0f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut flop_bet_counts = [0usize; NUM_EQUITY_BUCKETS];

        let mut turn_check_sums = [[0.0f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut turn_check_counts = [0usize; NUM_EQUITY_BUCKETS];
        let mut turn_bet_sums = [[0.0f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut turn_bet_counts = [0usize; NUM_EQUITY_BUCKETS];

        let mut river_check_sums = [[0.0f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut river_check_counts = [0usize; NUM_EQUITY_BUCKETS];
        let mut river_bet_sums = [[0.0f64; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut river_bet_counts = [0usize; NUM_EQUITY_BUCKETS];

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

            if let Some(pos) = key.find("F:B") {
                if key.len() >= pos + 5 {
                    if let Ok(b) = key[pos + 3..pos + 5].parse::<usize>() {
                        if b < NUM_EQUITY_BUCKETS {
                            if facing_bet {
                                for i in 0..HOLDEM_NUM_ACTIONS {
                                    flop_bet_sums[b][i] += vals[i];
                                }
                                flop_bet_counts[b] += 1;
                            } else {
                                for i in 0..HOLDEM_NUM_ACTIONS {
                                    flop_check_sums[b][i] += vals[i];
                                }
                                flop_check_counts[b] += 1;
                            }
                        }
                    }
                }
            } else if let Some(pos) = key.find("T:B") {
                if key.len() >= pos + 5 {
                    if let Ok(b) = key[pos + 3..pos + 5].parse::<usize>() {
                        if b < NUM_EQUITY_BUCKETS {
                            if facing_bet {
                                for i in 0..HOLDEM_NUM_ACTIONS {
                                    turn_bet_sums[b][i] += vals[i];
                                }
                                turn_bet_counts[b] += 1;
                            } else {
                                for i in 0..HOLDEM_NUM_ACTIONS {
                                    turn_check_sums[b][i] += vals[i];
                                }
                                turn_check_counts[b] += 1;
                            }
                        }
                    }
                }
            } else if let Some(pos) = key.find("R:B") {
                if key.len() >= pos + 5 {
                    if let Ok(b) = key[pos + 3..pos + 5].parse::<usize>() {
                        if b < NUM_EQUITY_BUCKETS {
                            if facing_bet {
                                for i in 0..HOLDEM_NUM_ACTIONS {
                                    river_bet_sums[b][i] += vals[i];
                                }
                                river_bet_counts[b] += 1;
                            } else {
                                for i in 0..HOLDEM_NUM_ACTIONS {
                                    river_check_sums[b][i] += vals[i];
                                }
                                river_check_counts[b] += 1;
                            }
                        }
                    }
                }
            } else if key.starts_with("P:") && !key.contains('|') {
                if let Some(slash_idx) = key.find('/') {
                    let hand_name = &key[2..slash_idx];
                    let sum_entry = preflop_sums
                        .entry(hand_name.to_string())
                        .or_insert([0.0; HOLDEM_NUM_ACTIONS]);
                    for i in 0..HOLDEM_NUM_ACTIONS {
                        sum_entry[i] += vals[i];
                    }
                    let count_entry = preflop_counts.entry(hand_name.to_string()).or_insert(0);
                    *count_entry += 1;
                }
            }
        }

        let mut preflop_avg = HashMap::with_capacity(preflop_sums.len());
        for (name, sum_arr) in preflop_sums {
            let count = preflop_counts.get(&name).copied().unwrap_or(1).max(1) as f64;
            let mut avg_arr = [0.0; HOLDEM_NUM_ACTIONS];
            for i in 0..HOLDEM_NUM_ACTIONS {
                avg_arr[i] = sum_arr[i] / count;
            }
            preflop_avg.insert(name, avg_arr);
        }

        let mut flop_check_avg = [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut flop_check_valid = [false; NUM_EQUITY_BUCKETS];
        let mut flop_bet_avg = [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut flop_bet_valid = [false; NUM_EQUITY_BUCKETS];
        for b in 0..NUM_EQUITY_BUCKETS {
            if flop_check_counts[b] > 0 {
                let n = flop_check_counts[b] as f64;
                for i in 0..HOLDEM_NUM_ACTIONS {
                    flop_check_avg[b][i] = flop_check_sums[b][i] / n;
                }
                flop_check_valid[b] = true;
            }
            if flop_bet_counts[b] > 0 {
                let n = flop_bet_counts[b] as f64;
                for i in 0..HOLDEM_NUM_ACTIONS {
                    flop_bet_avg[b][i] = flop_bet_sums[b][i] / n;
                }
                flop_bet_valid[b] = true;
            }
        }

        let mut turn_check_avg = [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut turn_check_valid = [false; NUM_EQUITY_BUCKETS];
        let mut turn_bet_avg = [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut turn_bet_valid = [false; NUM_EQUITY_BUCKETS];
        for b in 0..NUM_EQUITY_BUCKETS {
            if turn_check_counts[b] > 0 {
                let n = turn_check_counts[b] as f64;
                for i in 0..HOLDEM_NUM_ACTIONS {
                    turn_check_avg[b][i] = turn_check_sums[b][i] / n;
                }
                turn_check_valid[b] = true;
            }
            if turn_bet_counts[b] > 0 {
                let n = turn_bet_counts[b] as f64;
                for i in 0..HOLDEM_NUM_ACTIONS {
                    turn_bet_avg[b][i] = turn_bet_sums[b][i] / n;
                }
                turn_bet_valid[b] = true;
            }
        }

        let mut river_check_avg = [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut river_check_valid = [false; NUM_EQUITY_BUCKETS];
        let mut river_bet_avg = [[0.0; HOLDEM_NUM_ACTIONS]; NUM_EQUITY_BUCKETS];
        let mut river_bet_valid = [false; NUM_EQUITY_BUCKETS];
        for b in 0..NUM_EQUITY_BUCKETS {
            if river_check_counts[b] > 0 {
                let n = river_check_counts[b] as f64;
                for i in 0..HOLDEM_NUM_ACTIONS {
                    river_check_avg[b][i] = river_check_sums[b][i] / n;
                }
                river_check_valid[b] = true;
            }
            if river_bet_counts[b] > 0 {
                let n = river_bet_counts[b] as f64;
                for i in 0..HOLDEM_NUM_ACTIONS {
                    river_bet_avg[b][i] = river_bet_sums[b][i] / n;
                }
                river_bet_valid[b] = true;
            }
        }

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
        if round == 1 {
            let (_, name) = preflop_bucket(hole[0], hole[1]);
            self.preflop_avg.get(name).copied()
        } else {
            let bucket = postflop_equity_bucket(hole, board);
            if bucket >= NUM_EQUITY_BUCKETS {
                return None;
            }
            let facing_bet = to_call > 0;
            match round {
                2 => {
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
                3 => {
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
                4 => {
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

    pub fn get_blended_fallback(
        &self,
        hole: &[Card; 2],
        board: &[Card],
        round: u8,
        to_call: i32,
        pot: i32,
        legal: &[u8],
    ) -> Vec<f64> {
        let fallback = get_holdem_fallback_strategy(hole, board, round, to_call, pot, legal);
        if let Some(avg) = self.query_bucket_avg(round, hole, board, to_call) {
            let mut blended = vec![0.0f64; HOLDEM_NUM_ACTIONS];
            let mut sum = 0.0f64;
            for &a in legal {
                let idx = a as usize;
                if idx < HOLDEM_NUM_ACTIONS {
                    let val = 0.60 * avg[idx] + 0.40 * fallback[idx];
                    blended[idx] = val.max(0.0);
                    sum += blended[idx];
                }
            }
            if sum > 1e-9 {
                let inv = 1.0 / sum;
                for &a in legal {
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
