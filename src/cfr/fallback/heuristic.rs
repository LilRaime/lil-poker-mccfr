/*!
 * Intelligent GTO-aligned Strategy Fallback and Equity Heuristic Engine.
 * Used when an Information Set key is not present in the offline strategy model.
 */

use crate::cfr::abstraction::{
    BUCKET_SECOND_NUT_FLUSH, BUCKET_STRAIGHT_MIN, BUCKET_STRONG_DRAW_MIN, BUCKET_STRONG_FLUSH_MIN,
    BUCKET_TOP_PAIR_MIN, BUCKET_TRIPS_MIN, BUCKET_TWO_PAIR_MIN, BUCKET_WEAK_DRAW_MIN,
    BUCKET_WEAK_PAIR_MIN, postflop_equity_bucket, preflop_bucket, preflop_strength_tier,
};
use crate::game::config::{
    BIG_BLIND, HOLDEM_NUM_ACTIONS, NUM_EQUITY_BUCKETS, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER,
    ROUND_TURN, SMALL_BLIND,
};
use crate::game::holdem::{
    ALL_IN, CALL_CHECK, Card, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, Rank, Suit,
};

#[derive(Debug, Clone, Copy)]
pub struct FallbackRequest<'a> {
    pub hole: [Card; 2],
    pub board: &'a [Card],
    pub round: u8,
    pub to_call: i32,
    pub pot: i32,
    pub legal: &'a [u8],
    pub table_bb: i32,
}

impl Default for FallbackRequest<'_> {
    fn default() -> Self {
        Self {
            hole: [
                Card::new(Rank::Ace, Suit::Spades),
                Card::new(Rank::King, Suit::Spades),
            ],
            board: &[],
            round: ROUND_PREFLOP,
            to_call: 0,
            pot: 0,
            legal: &[],
            table_bb: BIG_BLIND,
        }
    }
}

/**
 * Returns a robust probability distribution `[p_fold, p_call, p_raise_min, p_raise_third_pot, p_raise_half_pot, p_all_in]`
 * tailored to hand strength, equity tier, pot odds, and legal actions.
 */
pub fn get_holdem_fallback_strategy(req: &FallbackRequest<'_>) -> [f64; HOLDEM_NUM_ACTIONS] {
    let raw = if req.round == ROUND_PREFLOP {
        let (pf_idx, _) = preflop_bucket(req.hole[0], req.hole[1]);
        preflop_fallback(pf_idx, req.to_call, req.pot, req.table_bb)
    } else {
        let bucket = postflop_equity_bucket(&req.hole, req.board);
        postflop_fallback(bucket, req.round, req.to_call, req.pot)
    };

    normalize_legal_action_probs(&raw, req.legal)
}

/** Fast postflop fallback lookup when precomputed equity bucket is already known. */
#[inline]
pub fn get_postflop_fallback_by_bucket(
    bucket: usize,
    round: u8,
    to_call: i32,
    pot: i32,
    legal: &[u8],
) -> [f64; HOLDEM_NUM_ACTIONS] {
    let raw = postflop_fallback(bucket, round, to_call, pot);
    normalize_legal_action_probs(&raw, legal)
}

#[inline]
fn normalize_legal_action_probs(
    raw: &[f64; HOLDEM_NUM_ACTIONS],
    legal: &[u8],
) -> [f64; HOLDEM_NUM_ACTIONS] {
    /* Zero out illegal actions and normalize */
    let mut filtered = [0.0f64; HOLDEM_NUM_ACTIONS];
    let mut sum = 0.0f64;

    for &a in legal {
        let p = raw[a as usize].max(0.0);
        filtered[a as usize] = p;
        sum += p;
    }

    if sum > 1e-9 {
        let inv_sum = 1.0 / sum;
        for p in filtered.iter_mut() {
            *p *= inv_sum;
        }
    } else if !legal.is_empty() {
        /* Uniform over legal actions if zero sum */
        let u = 1.0 / legal.len() as f64;
        for &a in legal {
            filtered[a as usize] = u;
        }
    }

    filtered
}

/** Preflop heuristic based on Sklansky-Chubukov / canonical 169 hand tiers (1..=8) */
fn preflop_fallback(
    idx: usize,
    to_call: i32,
    pot: i32,
    table_bb: i32,
) -> [f64; HOLDEM_NUM_ACTIONS] {
    let mut s = [0.0f64; HOLDEM_NUM_ACTIONS];
    let tier = preflop_strength_tier(idx);
    let eff_bb = if table_bb > 0 && table_bb != BIG_BLIND {
        table_bb
    } else if pot > 0 && pot < BIG_BLIND + SMALL_BLIND {
        ((pot - to_call).max(1) * 2 / 3).max(1)
    } else {
        BIG_BLIND
    };
    let pf_tier1_call_max = eff_bb * 8;
    let pf_tier2_call_max = eff_bb * 4;
    let pf_tier3_cheap_call_max = eff_bb * 2;
    let pf_tier3_fold_boundary = eff_bb * 6;
    let pf_tier4_call_max = eff_bb * 2;

    match tier {
        1 => {
            /* Tier 1: Super-Premium Monsters (AA, KK, QQ, AKs, AKo) */
            if to_call == 0 {
                s[CALL_CHECK as usize] = 0.05;
                s[RAISE_MIN as usize] = 0.15;
                s[RAISE_THIRD_POT as usize] = 0.35;
                s[RAISE_HALF_POT as usize] = 0.40;
                s[ALL_IN as usize] = 0.05;
            } else if to_call <= pf_tier1_call_max {
                s[CALL_CHECK as usize] = 0.30;
                s[RAISE_MIN as usize] = 0.15;
                s[RAISE_THIRD_POT as usize] = 0.20;
                s[RAISE_HALF_POT as usize] = 0.25;
                s[ALL_IN as usize] = 0.10;
            } else {
                /* Facing 3-bet/4-bet shove: Call or All-in */
                s[CALL_CHECK as usize] = 0.40;
                s[ALL_IN as usize] = 0.60;
            }
        }
        2 => {
            /* Tier 2: Strong Premium (JJ, TT, 99, AQs, AJs, KQs, AQo) */
            if to_call == 0 {
                s[CALL_CHECK as usize] = 0.15;
                s[RAISE_MIN as usize] = 0.25;
                s[RAISE_THIRD_POT as usize] = 0.35;
                s[RAISE_HALF_POT as usize] = 0.25;
            } else if to_call <= (eff_bb / 2).max(1) {
                /* HU SB open with Tier 2: raise predominantly instead of passive limping */
                s[CALL_CHECK as usize] = 0.15;
                s[RAISE_MIN as usize] = 0.45;
                s[RAISE_THIRD_POT as usize] = 0.30;
                s[RAISE_HALF_POT as usize] = 0.10;
            } else if to_call <= pf_tier2_call_max {
                s[CALL_CHECK as usize] = 0.65;
                s[RAISE_MIN as usize] = 0.15;
                s[RAISE_THIRD_POT as usize] = 0.15;
                s[RAISE_HALF_POT as usize] = 0.05;
            } else {
                s[FOLD as usize] = 0.35;
                s[CALL_CHECK as usize] = 0.50;
                s[ALL_IN as usize] = 0.15;
            }
        }
        3 | 4 => {
            /* Tier 3-4: Upper Playable & Solid Open/Call (88-55, ATs-A2s, suited broadways/connectors, AJo-A9o, KQo-KTo, QJo) */
            if to_call == 0 {
                s[CALL_CHECK as usize] = 0.45;
                s[RAISE_MIN as usize] = 0.25;
                s[RAISE_THIRD_POT as usize] = 0.25;
                s[RAISE_HALF_POT as usize] = 0.05;
            } else if to_call <= pf_tier3_cheap_call_max {
                s[CALL_CHECK as usize] = 0.70;
                s[RAISE_MIN as usize] = 0.10;
                s[RAISE_THIRD_POT as usize] = 0.05;
                s[FOLD as usize] = 0.15;
            } else if to_call <= pf_tier3_fold_boundary {
                s[CALL_CHECK as usize] = 0.40;
                s[FOLD as usize] = 0.60;
            } else {
                /* Facing 3-bet/4-bet shove: Fold mid pairs & suited cards against massive shoves */
                s[FOLD as usize] = 0.85;
                s[CALL_CHECK as usize] = 0.15;
            }
        }
        5 | 6 => {
            /* Tier 5-6: Speculative / Late-Position Open / BB Defense (44-22, suited kings/queens/gappers, offsuit aces/connectors) */
            if to_call == 0 {
                s[CALL_CHECK as usize] = 0.85;
                s[RAISE_MIN as usize] = 0.15;
            } else if to_call <= pf_tier4_call_max {
                s[CALL_CHECK as usize] = 0.40;
                s[FOLD as usize] = 0.60;
            } else {
                s[FOLD as usize] = 0.96;
                s[RAISE_THIRD_POT as usize] = 0.04;
            }
        }
        _ => {
            /* Tier 7-8: Borderline & Trash (72o, 83o, etc.) */
            if to_call == 0 {
                s[CALL_CHECK as usize] = 1.0;
            } else {
                s[FOLD as usize] = 0.98;
                s[RAISE_THIRD_POT as usize] = 0.02;
            }
        }
    }

    s
}

/** Postflop heuristic based on 88 equity buckets (0..=87), board round, pot odds, and aggression */
fn postflop_fallback(
    bucket: usize,
    round: u8,
    to_call: i32,
    pot: i32,
) -> [f64; HOLDEM_NUM_ACTIONS] {
    if to_call == 0 {
        return postflop_checked_to_fallback(bucket, round);
    }
    let total_pot = (pot + to_call).max(1);
    let pot_odds = to_call as f64 / total_pot as f64;
    postflop_facing_bet_fallback(bucket, round, pot_odds)
}

fn postflop_checked_to_fallback(bucket: usize, round: u8) -> [f64; HOLDEM_NUM_ACTIONS] {
    let mut s = [0.0f64; HOLDEM_NUM_ACTIONS];
    if bucket >= BUCKET_SECOND_NUT_FLUSH {
        s[CALL_CHECK as usize] = 0.10;
        s[RAISE_THIRD_POT as usize] = 0.20;
        s[RAISE_HALF_POT as usize] = 0.45;
        s[ALL_IN as usize] = 0.25;
    } else if bucket >= BUCKET_STRAIGHT_MIN {
        s[CALL_CHECK as usize] = 0.18;
        s[RAISE_MIN as usize] = 0.07;
        s[RAISE_THIRD_POT as usize] = 0.35;
        s[RAISE_HALF_POT as usize] = 0.30;
        s[ALL_IN as usize] = 0.10;
    } else if bucket >= BUCKET_TRIPS_MIN {
        s[CALL_CHECK as usize] = 0.15;
        s[RAISE_MIN as usize] = 0.05;
        s[RAISE_THIRD_POT as usize] = 0.30;
        s[RAISE_HALF_POT as usize] = 0.35;
        s[ALL_IN as usize] = 0.15;
    } else if bucket >= BUCKET_TWO_PAIR_MIN {
        s[CALL_CHECK as usize] = 0.18;
        s[RAISE_MIN as usize] = 0.12;
        s[RAISE_THIRD_POT as usize] = 0.35;
        s[RAISE_HALF_POT as usize] = 0.27;
        s[ALL_IN as usize] = 0.08;
    } else if bucket >= BUCKET_TOP_PAIR_MIN {
        if round == ROUND_FLOP {
            s[CALL_CHECK as usize] = 0.30;
            s[RAISE_MIN as usize] = 0.25;
            s[RAISE_THIRD_POT as usize] = 0.30;
            s[RAISE_HALF_POT as usize] = 0.15;
        } else {
            /* Turn/River: RAISE_HALF_POT is a 150% overbet, so lean toward 33%/60% with 1-pair hands */
            s[CALL_CHECK as usize] = 0.38;
            s[RAISE_MIN as usize] = 0.24;
            s[RAISE_THIRD_POT as usize] = 0.30;
            s[RAISE_HALF_POT as usize] = 0.08;
        }
    } else if round < ROUND_RIVER && (bucket == 33 || bucket == 38) {
        s[CALL_CHECK as usize] = 0.72;
        s[RAISE_MIN as usize] = 0.18;
        s[RAISE_THIRD_POT as usize] = 0.10;
    } else if bucket >= BUCKET_WEAK_PAIR_MIN {
        s[CALL_CHECK as usize] = 0.90;
        s[RAISE_MIN as usize] = 0.08;
        s[RAISE_THIRD_POT as usize] = 0.02;
    } else if round < ROUND_RIVER && bucket >= BUCKET_STRONG_DRAW_MIN {
        s[RAISE_MIN as usize] = 0.10;
        s[RAISE_THIRD_POT as usize] = 0.30;
        s[RAISE_HALF_POT as usize] = 0.20;
        if round == ROUND_FLOP || bucket >= 24 {
            s[CALL_CHECK as usize] = 0.35;
            s[ALL_IN as usize] = 0.05;
        } else {
            s[CALL_CHECK as usize] = 0.40;
        }
    } else if round < ROUND_RIVER && bucket >= BUCKET_WEAK_DRAW_MIN {
        s[CALL_CHECK as usize] = 0.78;
        s[RAISE_MIN as usize] = 0.15;
        s[RAISE_THIRD_POT as usize] = 0.07;
    } else {
        s[CALL_CHECK as usize] = 0.88;
        s[RAISE_MIN as usize] = 0.08;
        s[RAISE_THIRD_POT as usize] = 0.04;
    }
    s
}

fn postflop_facing_bet_fallback(
    bucket: usize,
    round: u8,
    pot_odds: f64,
) -> [f64; HOLDEM_NUM_ACTIONS] {
    if round < ROUND_RIVER && (BUCKET_STRONG_DRAW_MIN..BUCKET_WEAK_PAIR_MIN).contains(&bucket) {
        return strong_draw_facing_bet_fallback(bucket, round, pot_odds);
    }
    let mut s = [0.0f64; HOLDEM_NUM_ACTIONS];
    if bucket >= BUCKET_SECOND_NUT_FLUSH {
        s[CALL_CHECK as usize] = 0.25;
        s[RAISE_HALF_POT as usize] = 0.35;
        s[ALL_IN as usize] = 0.40;
    } else if bucket == BUCKET_STRAIGHT_MIN {
        if pot_odds <= 0.28 {
            s[CALL_CHECK as usize] = 0.85;
            s[FOLD as usize] = 0.15;
        } else {
            s[CALL_CHECK as usize] = 0.35;
            s[FOLD as usize] = 0.65;
        }
    } else if bucket >= 74 {
        if bucket >= BUCKET_STRONG_FLUSH_MIN && pot_odds > 0.35 {
            s[CALL_CHECK as usize] = 0.85;
            s[RAISE_HALF_POT as usize] = 0.15;
        } else if pot_odds <= 0.35 {
            s[CALL_CHECK as usize] = 0.60;
            s[RAISE_HALF_POT as usize] = 0.25;
            s[ALL_IN as usize] = 0.15;
        } else {
            s[CALL_CHECK as usize] = 0.70;
            s[FOLD as usize] = 0.20;
            s[RAISE_HALF_POT as usize] = 0.10;
        }
    } else if bucket >= BUCKET_TRIPS_MIN {
        s[CALL_CHECK as usize] = 0.45;
        s[RAISE_HALF_POT as usize] = 0.30;
        s[ALL_IN as usize] = 0.25;
    } else if bucket >= BUCKET_TWO_PAIR_MIN {
        if pot_odds <= 0.35 {
            s[CALL_CHECK as usize] = 0.55;
            s[RAISE_THIRD_POT as usize] = 0.20;
            s[RAISE_HALF_POT as usize] = 0.15;
            s[ALL_IN as usize] = 0.10;
        } else {
            s[CALL_CHECK as usize] = 0.65;
            s[RAISE_HALF_POT as usize] = 0.15;
            s[ALL_IN as usize] = 0.10;
            s[FOLD as usize] = 0.10;
        }
    } else if bucket >= BUCKET_TOP_PAIR_MIN {
        if pot_odds <= 0.30 {
            s[CALL_CHECK as usize] = 0.70;
            s[RAISE_HALF_POT as usize] = 0.15;
            s[RAISE_MIN as usize] = 0.10;
            s[FOLD as usize] = 0.05;
        } else {
            s[CALL_CHECK as usize] = 0.55;
            s[FOLD as usize] = 0.30;
            s[RAISE_HALF_POT as usize] = 0.15;
        }
    } else if round < ROUND_RIVER && (bucket == 33 || bucket == 38) {
        if pot_odds <= 0.35 {
            s[CALL_CHECK as usize] = 0.85;
            s[FOLD as usize] = 0.10;
            s[RAISE_THIRD_POT as usize] = 0.05;
        } else {
            s[CALL_CHECK as usize] = 0.35;
            s[FOLD as usize] = 0.65;
        }
    } else if bucket >= BUCKET_WEAK_PAIR_MIN {
        if pot_odds <= 0.20 {
            s[CALL_CHECK as usize] = 0.60;
            s[FOLD as usize] = 0.40;
        } else if pot_odds <= 0.29 {
            s[CALL_CHECK as usize] = 0.35;
            s[FOLD as usize] = 0.65;
        } else {
            s[CALL_CHECK as usize] = 0.12;
            s[FOLD as usize] = 0.88;
        }
    } else if round < ROUND_RIVER && bucket >= BUCKET_WEAK_DRAW_MIN {
        if pot_odds <= 0.18 {
            s[CALL_CHECK as usize] = 0.65;
            s[FOLD as usize] = 0.35;
        } else if pot_odds <= 0.24 {
            s[CALL_CHECK as usize] = 0.20;
            s[FOLD as usize] = 0.80;
        } else {
            s[FOLD as usize] = 1.0;
        }
    } else if round == ROUND_RIVER && bucket >= 8 {
        if pot_odds <= 0.18 {
            s[CALL_CHECK as usize] = 0.60;
            s[FOLD as usize] = 0.40;
        } else if pot_odds <= 0.28 {
            s[CALL_CHECK as usize] = 0.25;
            s[FOLD as usize] = 0.75;
        } else {
            s[FOLD as usize] = 0.98;
            s[CALL_CHECK as usize] = 0.02;
        }
    } else {
        s[FOLD as usize] = 0.99;
        s[RAISE_THIRD_POT as usize] = 0.01;
    }
    s
}

fn strong_draw_facing_bet_fallback(
    bucket: usize,
    round: u8,
    pot_odds: f64,
) -> [f64; HOLDEM_NUM_ACTIONS] {
    let mut s = [0.0f64; HOLDEM_NUM_ACTIONS];
    let max_call_odds = if round == ROUND_TURN {
        if bucket >= 24 {
            0.31
        } else if bucket >= 19 {
            0.22
        } else {
            0.19
        }
    } else if bucket >= 24 {
        0.42
    } else {
        0.28
    };

    if pot_odds <= max_call_odds {
        s[CALL_CHECK as usize] = 0.75;
        s[RAISE_THIRD_POT as usize] = if round == ROUND_FLOP { 0.15 } else { 0.05 };
        s[FOLD as usize] = if round == ROUND_FLOP { 0.10 } else { 0.20 };
    } else if round == ROUND_FLOP && pot_odds <= max_call_odds + 0.07 {
        s[CALL_CHECK as usize] = 0.45;
        s[FOLD as usize] = 0.50;
        s[RAISE_THIRD_POT as usize] = 0.05;
    } else {
        s[FOLD as usize] = 0.95;
        s[CALL_CHECK as usize] = 0.05;
    }
    s
}

/**
 * Multi-tier hierarchical information set lookup.
 * Guarantees high hit rates (>98%) for models trained with rich history.
 */
pub fn lookup_strategy_hierarchical<'a, V>(
    get_fn: impl Fn(&str) -> Option<&'a V>,
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &(impl crate::cfr::abstraction::HistoryActions + ?Sized),
) -> Option<&'a V> {
    use crate::cfr::abstraction::{
        append_texture_to_key, compact_board_texture_char, get_holdem_infoset_key,
        get_holdem_infoset_key_rich, get_holdem_infoset_key_rich_with_bucket,
        get_holdem_infoset_key_with_bucket, postflop_equity_bucket,
    };

    let tex_opt = if round >= ROUND_FLOP && !board.is_empty() {
        Some(compact_board_texture_char(board))
    } else {
        None
    };
    let try_get = |k: &str| -> Option<&'a V> {
        if let Some(tex) = tex_opt {
            let tk = append_texture_to_key(k, tex);
            if let Some(v) = get_fn(&tk) {
                return Some(v);
            }
        }
        get_fn(k)
    };

    /* 1. Exact rich key (try v2 board-textured key first, then untextured v1 key) */
    let rich_key = get_holdem_infoset_key_rich(hole, board, round, history);
    let exact_key = get_holdem_infoset_key(hole, board, round, history);
    if let Some(v) = try_get(&rich_key) {
        return Some(v);
    }

    /* 2. Exact compact key (for non-rich models, e.g. "F:B36/c") */
    if let Some(v) = try_get(&exact_key) {
        return Some(v);
    }

    if round >= ROUND_FLOP {
        let bucket = postflop_equity_bucket(hole, board) as i32;

        /* 3. Adjacent bucket search (+1, -1, +2, -2) preserving exact street history and active player */
        for delta in [1i32, -1, 2, -2] {
            let nb = bucket + delta;
            if (0..NUM_EQUITY_BUCKETS as i32).contains(&nb) {
                let adj_rich =
                    get_holdem_infoset_key_rich_with_bucket(hole, nb as u8, round, history);
                if let Some(v) = try_get(&adj_rich) {
                    return Some(v);
                }
                let adj_exact = get_holdem_infoset_key_with_bucket(hole, nb as u8, round, history);
                if let Some(v) = try_get(&adj_exact) {
                    return Some(v);
                }
            }
        }

        /* 4. Canonical preflop line with exact current street action (same player turn) */
        if round == ROUND_FLOP {
            let rc_key = format!("P:rc|{exact_key}");
            if let Some(v) = try_get(&rc_key) {
                return Some(v);
            }
            let cc_key = format!("P:cc|{exact_key}");
            if let Some(v) = try_get(&cc_key) {
                return Some(v);
            }
        } else if round == ROUND_TURN {
            let rc_f_c = format!("P:rc|F:cc|{exact_key}");
            if let Some(v) = try_get(&rc_f_c) {
                return Some(v);
            }
            let rc_f_rc = format!("P:rc|F:rc|{exact_key}");
            if let Some(v) = try_get(&rc_f_rc) {
                return Some(v);
            }
            let cc_f_c = format!("P:cc|F:cc|{exact_key}");
            if let Some(v) = try_get(&cc_f_c) {
                return Some(v);
            }
        }

        /* 5. Even-step street action truncation (steps of 2 ONLY, and preserving facing-bet parity
         * so a facing-bet state NEVER falls back to a check-node with 0% fold probability!) */
        if let Some(slash_idx) = exact_key.find('/') {
            let street_prefix = &exact_key[..slash_idx];
            let acts = &exact_key[slash_idx + 1..];
            let orig_facing_bet = acts.chars().last().is_some_and(|c| c != 'c');
            let mut end = acts.len();
            while end >= 2 {
                end -= 2;
                let sub_acts = &acts[..end];
                let sub_facing_bet = sub_acts.chars().last().is_some_and(|c| c != 'c');
                if orig_facing_bet != sub_facing_bet {
                    continue;
                }
                let truncated_exact = format!("{street_prefix}/{sub_acts}");
                if round == ROUND_TURN {
                    let t_rc_f_c = format!("P:rc|F:cc|{truncated_exact}");
                    if let Some(v) = try_get(&t_rc_f_c) {
                        return Some(v);
                    }
                }
                let t_rc = format!("P:rc|{truncated_exact}");
                if let Some(v) = try_get(&t_rc) {
                    return Some(v);
                }
                let t_cc = format!("P:cc|{truncated_exact}");
                if let Some(v) = try_get(&t_cc) {
                    return Some(v);
                }
                if let Some(v) = try_get(&truncated_exact) {
                    return Some(v);
                }
            }
        }
    }

    None
}

pub fn lookup_strategy_hierarchical_owned<V>(
    get_fn: impl Fn(&str) -> Option<V>,
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &(impl crate::cfr::abstraction::HistoryActions + ?Sized),
) -> Option<V> {
    use crate::cfr::abstraction::{
        append_texture_to_key, compact_board_texture_char, get_holdem_infoset_key,
        get_holdem_infoset_key_rich, get_holdem_infoset_key_rich_with_bucket,
        get_holdem_infoset_key_with_bucket, postflop_equity_bucket,
    };

    let tex_opt = if round >= ROUND_FLOP && !board.is_empty() {
        Some(compact_board_texture_char(board))
    } else {
        None
    };
    let try_get = |k: &str| -> Option<V> {
        if let Some(tex) = tex_opt {
            let tk = append_texture_to_key(k, tex);
            if let Some(v) = get_fn(&tk) {
                return Some(v);
            }
        }
        get_fn(k)
    };

    let rich_key = get_holdem_infoset_key_rich(hole, board, round, history);
    let exact_key = get_holdem_infoset_key(hole, board, round, history);
    if let Some(v) = try_get(&rich_key) {
        return Some(v);
    }

    if let Some(v) = try_get(&exact_key) {
        return Some(v);
    }

    if round >= ROUND_FLOP {
        let bucket = postflop_equity_bucket(hole, board) as i32;
        for delta in [1i32, -1, 2, -2] {
            let nb = bucket + delta;
            if (0..NUM_EQUITY_BUCKETS as i32).contains(&nb) {
                let adj_rich =
                    get_holdem_infoset_key_rich_with_bucket(hole, nb as u8, round, history);
                if let Some(v) = try_get(&adj_rich) {
                    return Some(v);
                }
                let adj_exact = get_holdem_infoset_key_with_bucket(hole, nb as u8, round, history);
                if let Some(v) = try_get(&adj_exact) {
                    return Some(v);
                }
            }
        }

        if round == ROUND_TURN {
            let rc_f_c = format!("P:rc|F:cc|{exact_key}");
            if let Some(v) = try_get(&rc_f_c) {
                return Some(v);
            }
            let rc_f_rc = format!("P:rc|F:rc|{exact_key}");
            if let Some(v) = try_get(&rc_f_rc) {
                return Some(v);
            }
            let cc_f_c = format!("P:cc|F:cc|{exact_key}");
            if let Some(v) = try_get(&cc_f_c) {
                return Some(v);
            }
        }
        let rc_key = format!("P:rc|{exact_key}");
        if let Some(v) = try_get(&rc_key) {
            return Some(v);
        }
        let cc_key = format!("P:cc|{exact_key}");
        if let Some(v) = try_get(&cc_key) {
            return Some(v);
        }

        if let Some(slash_idx) = exact_key.find('/') {
            let street_prefix = &exact_key[..slash_idx];
            let acts = &exact_key[slash_idx + 1..];
            let orig_facing_bet = acts.chars().last().is_some_and(|c| c != 'c');
            let mut end = acts.len();
            while end >= 2 {
                end -= 2;
                let sub_acts = &acts[..end];
                let sub_facing_bet = sub_acts.chars().last().is_some_and(|c| c != 'c');
                if orig_facing_bet != sub_facing_bet {
                    continue;
                }
                let truncated_exact = format!("{street_prefix}/{sub_acts}");
                if round == ROUND_TURN {
                    let t_rc_f_c = format!("P:rc|F:cc|{truncated_exact}");
                    if let Some(v) = try_get(&t_rc_f_c) {
                        return Some(v);
                    }
                }
                let t_rc = format!("P:rc|{truncated_exact}");
                if let Some(v) = try_get(&t_rc) {
                    return Some(v);
                }
                let t_cc = format!("P:cc|{truncated_exact}");
                if let Some(v) = try_get(&t_cc) {
                    return Some(v);
                }
                if let Some(v) = try_get(&truncated_exact) {
                    return Some(v);
                }
            }
        }
    }

    None
}

/** Stack-aware hierarchical lookup for unified single-file multi-stack models:
 * tries stack-depth prefixed keys ("S15|" for <= 20 BB, "S30|" for 21..=45 BB) first,
 * and seamlessly falls back to unprefixed 100 BB blueprint keys if absent. */
pub fn lookup_strategy_hierarchical_with_stack<'a, V>(
    get_fn: impl Fn(&str) -> Option<&'a V>,
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &(impl crate::cfr::abstraction::HistoryActions + ?Sized),
    eff_stack_bb: i32,
) -> Option<&'a V> {
    let prefix = crate::cfr::abstraction::stack_depth_prefix(eff_stack_bb);
    if !prefix.is_empty()
        && let Some(v) = lookup_strategy_hierarchical(
            |k| {
                let prefixed = format!("{prefix}{k}");
                get_fn(&prefixed)
            },
            hole,
            board,
            round,
            history,
        )
    {
        return Some(v);
    }
    lookup_strategy_hierarchical(get_fn, hole, board, round, history)
}

pub fn lookup_strategy_hierarchical_owned_with_stack<V>(
    get_fn: impl Fn(&str) -> Option<V>,
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    history: &(impl crate::cfr::abstraction::HistoryActions + ?Sized),
    eff_stack_bb: i32,
) -> Option<V> {
    let prefix = crate::cfr::abstraction::stack_depth_prefix(eff_stack_bb);
    if !prefix.is_empty()
        && let Some(v) = lookup_strategy_hierarchical_owned(
            |k| {
                let prefixed = format!("{prefix}{k}");
                get_fn(&prefixed)
            },
            hole,
            board,
            round,
            history,
        )
    {
        return Some(v);
    }
    lookup_strategy_hierarchical_owned(get_fn, hole, board, round, history)
}
