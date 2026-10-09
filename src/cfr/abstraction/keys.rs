/*
 * Information set key formatting and zero-allocation 64-bit FNV-1a hashing.
 */

use super::postflop::postflop_equity_bucket;
use super::preflop::preflop_bucket;
use crate::game::holdem::{Card, RoundHistory};

pub trait HistoryActions {
    fn actions_in_round(&self, r_idx: usize) -> &[u8];
}

impl HistoryActions for [RoundHistory; 4] {
    #[inline(always)]
    fn actions_in_round(&self, r_idx: usize) -> &[u8] {
        &self[r_idx]
    }
}

impl HistoryActions for [Vec<u8>; 4] {
    #[inline(always)]
    fn actions_in_round(&self, r_idx: usize) -> &[u8] {
        &self[r_idx]
    }
}

#[inline(always)]
pub fn action_to_char(a: u8) -> char {
    match a {
        0 => 'f',
        1 => 'c',
        2 => 'r',
        3 => 't',
        4 => 'h',
        5 => 'a',
        _ => 'x',
    }
}

/* Fast zero-allocation infoset key formatting into a reusable String buffer */
#[inline]
pub fn format_holdem_infoset_key_slice(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    round_history: &[u8],
    out: &mut String,
) {
    out.clear();
    if round == 1 {
        let (_, name) = preflop_bucket(hole[0], hole[1]);
        out.push_str("P:");
        out.push_str(name);
        out.push('/');
    } else {
        let bucket = postflop_equity_bucket(hole, board);
        let round_code = match round {
            2 => "F:B",
            3 => "T:B",
            4 => "R:B",
            _ => "X:B",
        };
        out.push_str(round_code);
        if bucket < 10 {
            out.push('0');
        }
        use std::fmt::Write;
        let _ = write!(out, "{}", bucket);
        out.push('/');
    }
    for &a in round_history {
        out.push(action_to_char(a));
    }
}

#[inline]
pub fn format_holdem_infoset_key<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
    out: &mut String,
) {
    let r_idx = (round.saturating_sub(1)) as usize;
    format_holdem_infoset_key_slice(hole, board, round, history.actions_in_round(r_idx), out);
}

/* Generates compact abstracted Information Set key for Texas Hold'em */
pub fn get_holdem_infoset_key<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
) -> String {
    let mut s = String::with_capacity(16);
    format_holdem_infoset_key(hole, board, round, history, &mut s);
    s
}

/* Rich Information Set key formatting that retains previous-street action context (e.g. preflop 3-bet vs limp) */
#[inline]
pub fn format_holdem_infoset_key_rich<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
    out: &mut String,
) {
    out.clear();
    if round == 1 {
        let (_, name) = preflop_bucket(hole[0], hole[1]);
        out.push_str("P:");
        out.push_str(name);
        out.push('/');
        for &a in history.actions_in_round(0) {
            out.push(action_to_char(a));
        }
    } else {
        let pf_actions = history.actions_in_round(0);
        if !pf_actions.is_empty() {
            out.push_str("P:");
            for &a in pf_actions {
                out.push(action_to_char(a));
            }
            out.push('|');
        }
        let bucket = postflop_equity_bucket(hole, board);
        let round_code = match round {
            2 => "F:B",
            3 => "T:B",
            4 => "R:B",
            _ => "X:B",
        };
        out.push_str(round_code);
        if bucket < 10 {
            out.push('0');
        }
        use std::fmt::Write;
        let _ = write!(out, "{}", bucket);
        out.push('/');
        let r_idx = (round.saturating_sub(1)) as usize;
        for &a in history.actions_in_round(r_idx) {
            out.push(action_to_char(a));
        }
    }
}

pub fn get_holdem_infoset_key_rich<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
) -> String {
    let mut s = String::with_capacity(24);
    format_holdem_infoset_key_rich(hole, board, round, history, &mut s);
    s
}

/* ── Fast 64-bit Hash Infoset Keys (Zero-allocation FNV-1a) ───────────────── */

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[inline(always)]
fn fnv1a_combine(mut hash: u64, byte: u8) -> u64 {
    hash ^= byte as u64;
    hash.wrapping_mul(FNV_PRIME)
}

#[inline]
pub fn hash_holdem_infoset_key_slice(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    round_history: &[u8],
) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    h = fnv1a_combine(h, round);
    h = fnv1a_combine(h, b':');
    if round == 1 {
        let (idx, _) = preflop_bucket(hole[0], hole[1]);
        h = fnv1a_combine(h, (idx & 0xff) as u8);
        h = fnv1a_combine(h, ((idx >> 8) & 0xff) as u8);
    } else {
        let bucket = postflop_equity_bucket(hole, board);
        h = fnv1a_combine(h, bucket as u8);
    }
    h = fnv1a_combine(h, b'/');
    for &a in round_history {
        h = fnv1a_combine(h, a);
    }
    h
}

#[inline]
pub fn hash_holdem_infoset_key<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
) -> u64 {
    let r_idx = (round.saturating_sub(1)) as usize;
    hash_holdem_infoset_key_slice(hole, board, round, history.actions_in_round(r_idx))
}

#[inline]
pub fn hash_holdem_infoset_key_rich<H: HistoryActions + ?Sized>(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &H,
) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    if round == 1 {
        h = fnv1a_combine(h, 1);
        h = fnv1a_combine(h, b':');
        let (idx, _) = preflop_bucket(hole[0], hole[1]);
        h = fnv1a_combine(h, (idx & 0xff) as u8);
        h = fnv1a_combine(h, ((idx >> 8) & 0xff) as u8);
        h = fnv1a_combine(h, b'/');
        for &a in history.actions_in_round(0) {
            h = fnv1a_combine(h, a);
        }
    } else {
        let pf_actions = history.actions_in_round(0);
        if !pf_actions.is_empty() {
            h = fnv1a_combine(h, b'P');
            for &a in pf_actions {
                h = fnv1a_combine(h, a);
            }
            h = fnv1a_combine(h, b'|');
        }
        h = fnv1a_combine(h, round);
        h = fnv1a_combine(h, b':');
        let bucket = postflop_equity_bucket(hole, board);
        h = fnv1a_combine(h, bucket as u8);
        h = fnv1a_combine(h, b'/');
        let r_idx = (round.saturating_sub(1)) as usize;
        for &a in history.actions_in_round(r_idx) {
            h = fnv1a_combine(h, a);
        }
    }
    h
}
