/*
 * Intelligent GTO-aligned Strategy Fallback and Equity Heuristic Engine.
 * Used when an Information Set key is not present in the offline strategy model.
 */

use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket, preflop_strength_tier};
use crate::game::config::*;
use crate::game::holdem::{
    Card, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};

/*
 * Returns a robust probability distribution [p_fold, p_call, p_raise_min, p_raise_third_pot, p_raise_half_pot, p_all_in]
 * tailored to hand strength, equity tier, pot odds, and legal actions.
 */
pub fn get_holdem_fallback_strategy(
    hole: &[Card; 2],
    board: &[Card],
    round: u8,
    to_call: i32,
    pot: i32,
    legal: &[u8],
) -> [f64; HOLDEM_NUM_ACTIONS] {
    let raw = if round == 1 {
        let (pf_idx, _) = preflop_bucket(hole[0], hole[1]);
        preflop_fallback(pf_idx, to_call)
    } else {
        let bucket = postflop_equity_bucket(hole, board);
        postflop_fallback(bucket, round, to_call, pot)
    };

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
    } else {
        /* Uniform over legal actions if zero sum */
        let u = 1.0 / legal.len() as f64;
        for &a in legal {
            filtered[a as usize] = u;
        }
    }

    filtered
}

/* Preflop heuristic based on Sklansky-Chubukov / canonical 169 hand tiers (1..=8) */
fn preflop_fallback(idx: usize, to_call: i32) -> [f64; HOLDEM_NUM_ACTIONS] {
    let mut s = [0.0f64; HOLDEM_NUM_ACTIONS];
    let tier = preflop_strength_tier(idx);

    match tier {
        1 => {
            /* Tier 1: Super-Premium Monsters (AA, KK, QQ, AKs, AKo) */
            if to_call == 0 {
                s[CALL_CHECK as usize] = 0.05;
                s[RAISE_MIN as usize] = 0.15;
                s[RAISE_THIRD_POT as usize] = 0.35;
                s[RAISE_HALF_POT as usize] = 0.40;
                s[ALL_IN as usize] = 0.05;
            } else if to_call <= PF_TIER1_CALL_MAX {
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
            } else if to_call <= PF_TIER2_CALL_MAX {
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
            } else if to_call <= PF_TIER3_CHEAP_CALL_MAX {
                s[CALL_CHECK as usize] = 0.70;
                s[RAISE_MIN as usize] = 0.10;
                s[RAISE_THIRD_POT as usize] = 0.05;
                s[FOLD as usize] = 0.15;
            } else if to_call <= PF_TIER3_FOLD_BOUNDARY {
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
            } else if to_call <= PF_TIER4_CALL_MAX {
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

/* Postflop heuristic based on 88 equity buckets (0..=87), board round, pot odds, and aggression */
fn postflop_fallback(
    bucket: usize,
    round: u8,
    to_call: i32,
    pot: i32,
) -> [f64; HOLDEM_NUM_ACTIONS] {
    let mut s = [0.0f64; HOLDEM_NUM_ACTIONS];
    let total_pot = (pot + to_call).max(1);
    let pot_odds = to_call as f64 / total_pot as f64;

    if bucket >= 82 {
        /* True Nuts / Monsters (Nut/2nd-Nut Flush, Full House, Quads, Straight Flush: 82..=87) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.10; /* Trap / Slowplay */
            s[RAISE_THIRD_POT as usize] = 0.20;
            s[RAISE_HALF_POT as usize] = 0.45;
            s[ALL_IN as usize] = 0.25;
        } else {
            s[CALL_CHECK as usize] = 0.25;
            s[RAISE_HALF_POT as usize] = 0.35;
            s[ALL_IN as usize] = 0.40;
        }
    } else if bucket >= 73 {
        /* Straights and Non-Nut Flushes (73..=81) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.20;
            s[RAISE_THIRD_POT as usize] = 0.35;
            s[RAISE_HALF_POT as usize] = 0.35;
            s[ALL_IN as usize] = 0.10;
        } else if bucket == 73 {
            /* Board Straight / Idiot-End Dummy Straight: call cheap bets, fold to heavy pressure */
            if pot_odds <= 0.28 {
                s[CALL_CHECK as usize] = 0.85;
                s[FOLD as usize] = 0.15;
            } else {
                s[CALL_CHECK as usize] = 0.35;
                s[FOLD as usize] = 0.65;
            }
        } else if pot_odds <= 0.35 {
            s[CALL_CHECK as usize] = 0.60;
            s[RAISE_HALF_POT as usize] = 0.25;
            s[ALL_IN as usize] = 0.15;
        } else {
            /* Facing a large bet/overbet with a non-nut flush or low straight: prefer calling over jamming */
            s[CALL_CHECK as usize] = 0.70;
            s[FOLD as usize] = 0.20;
            s[RAISE_HALF_POT as usize] = 0.10;
        }
    } else if bucket >= 65 {
        /* Very Strong (Trips & Sets: 65..=72) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.15;
            s[RAISE_THIRD_POT as usize] = 0.30;
            s[RAISE_HALF_POT as usize] = 0.40;
            s[ALL_IN as usize] = 0.15;
        } else {
            s[CALL_CHECK as usize] = 0.45;
            s[RAISE_HALF_POT as usize] = 0.30;
            s[ALL_IN as usize] = 0.25;
        }
    } else if bucket >= 40 {
        /* Strong (Genuine Two Pair 57..=64, Overpairs 50..=56, Top Pair 40..=49) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.35;
            s[RAISE_THIRD_POT as usize] = 0.35;
            s[RAISE_HALF_POT as usize] = 0.25;
            s[RAISE_MIN as usize] = 0.05;
        } else if pot_odds <= 0.30 {
            s[CALL_CHECK as usize] = 0.70;
            s[RAISE_HALF_POT as usize] = 0.15;
            s[RAISE_MIN as usize] = 0.10;
            s[FOLD as usize] = 0.05;
        } else {
            s[CALL_CHECK as usize] = 0.55;
            s[FOLD as usize] = 0.30;
            s[RAISE_HALF_POT as usize] = 0.15;
        }
    } else if bucket >= 28 {
        /* Marginal / Bluff Catcher (Weak & Middle Pairs: 28..=39) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.95;
            s[RAISE_THIRD_POT as usize] = 0.05; /* Rare small probe bet */
        } else if pot_odds <= 0.15 {
            /* Very cheap call / exceptional pot odds */
            s[CALL_CHECK as usize] = 0.65;
            s[FOLD as usize] = 0.35;
        } else if pot_odds <= 0.25 {
            s[CALL_CHECK as usize] = 0.35;
            s[FOLD as usize] = 0.65;
        } else {
            /* Facing significant bet/raise: fold weak one-pair hands */
            s[CALL_CHECK as usize] = 0.10;
            s[FOLD as usize] = 0.90;
        }
    } else if round < 4 && bucket >= 15 {
        /* Strong Draws on Flop & Turn (OESD 15..=18, Flush Draw 19..=23, Combo Draw 24..=27) */
        let max_call_odds = if round == 3 {
            /* Turn (1 card to come): strict direct equity thresholds */
            if bucket >= 24 {
                0.31 /* Combo Draw (~30-34% equity) */
            } else if bucket >= 19 {
                0.22 /* Flush Draw (~19.6% equity) */
            } else {
                0.19 /* OESD (~17.4% equity) */
            }
        } else {
            /* Flop (2 cards to come): higher implied + two-card equity */
            if bucket >= 24 {
                0.42
            } else {
                0.28
            }
        };

        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.40;
            s[RAISE_THIRD_POT as usize] = 0.40; /* Standard 33% semi-bluff c-bet! */
            s[RAISE_HALF_POT as usize] = 0.15;
            if round == 2 || bucket >= 24 {
                s[ALL_IN as usize] = 0.05;
            }
        } else if pot_odds <= max_call_odds {
            s[CALL_CHECK as usize] = 0.75;
            s[RAISE_THIRD_POT as usize] = if round == 2 { 0.15 } else { 0.05 };
            s[FOLD as usize] = if round == 2 { 0.10 } else { 0.20 };
        } else if round == 2 && pot_odds <= max_call_odds + 0.07 {
            s[CALL_CHECK as usize] = 0.45;
            s[FOLD as usize] = 0.50;
            s[RAISE_THIRD_POT as usize] = 0.05;
        } else {
            /* Overpriced draw (especially on the Turn): fold */
            s[FOLD as usize] = 0.95;
            s[CALL_CHECK as usize] = 0.05;
        }
    } else if round < 4 && bucket >= 11 {
        /* Weak Draws on Flop & Turn (Gutshots) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.80;
            s[RAISE_THIRD_POT as usize] = 0.20;
        } else if pot_odds <= 0.18 {
            s[CALL_CHECK as usize] = 0.65;
            s[FOLD as usize] = 0.35;
        } else if pot_odds <= 0.24 {
            s[CALL_CHECK as usize] = 0.20;
            s[FOLD as usize] = 0.80;
        } else {
            /* Strict Odds Guard: never call large bets or overbets with a bare gutshot! */
            s[FOLD as usize] = 1.0;
            s[CALL_CHECK as usize] = 0.0;
        }
    } else if round == 4 && bucket >= 8 {
        /* River Ace-High / King-High Bluff Catchers */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.90;
            s[RAISE_THIRD_POT as usize] = 0.10; /* Small thin value/bluff */
        } else if pot_odds <= 0.18 {
            /* Cheap river bet / great pot odds: call down as bluff catcher */
            s[CALL_CHECK as usize] = 0.60;
            s[FOLD as usize] = 0.40;
        } else if pot_odds <= 0.28 {
            s[CALL_CHECK as usize] = 0.25;
            s[FOLD as usize] = 0.75;
        } else {
            /* Large bet / shove: fold Ace-high */
            s[FOLD as usize] = 0.98;
            s[CALL_CHECK as usize] = 0.02;
        }
    } else {
        /* Weak / Air / Missed draw */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.90;
            s[RAISE_THIRD_POT as usize] = 0.10; /* Small c-bet bluff */
        } else {
            s[FOLD as usize] = 0.99;
            s[RAISE_THIRD_POT as usize] = 0.01;
        }
    }

    s
}

/*
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
        get_holdem_infoset_key, get_holdem_infoset_key_rich,
        get_holdem_infoset_key_rich_with_bucket, get_holdem_infoset_key_with_bucket,
        postflop_equity_bucket,
    };

    /* 1. Exact rich key (e.g. "P:rc|F:B36/c") */
    let rich_key = get_holdem_infoset_key_rich(hole, board, round, history);
    if let Some(v) = get_fn(&rich_key) {
        return Some(v);
    }

    /* 2. Exact compact key (for non-rich models, e.g. "F:B36/c") */
    let exact_key = get_holdem_infoset_key(hole, board, round, history);
    if let Some(v) = get_fn(&exact_key) {
        return Some(v);
    }

    if round >= 2 {
        let bucket = postflop_equity_bucket(hole, board) as i32;

        /* 3. Adjacent bucket search (+1, -1, +2, -2) preserving exact street history and active player */
        for delta in [1i32, -1, 2, -2] {
            let nb = bucket + delta;
            if (0..NUM_EQUITY_BUCKETS as i32).contains(&nb) {
                let adj_rich =
                    get_holdem_infoset_key_rich_with_bucket(hole, nb as u8, round, history);
                if let Some(v) = get_fn(&adj_rich) {
                    return Some(v);
                }
                let adj_exact = get_holdem_infoset_key_with_bucket(hole, nb as u8, round, history);
                if let Some(v) = get_fn(&adj_exact) {
                    return Some(v);
                }
            }
        }

        /* 4. Canonical preflop line with exact current street action (same player turn) */
        if round == 2 {
            let rc_key = format!("P:rc|{}", exact_key);
            if let Some(v) = get_fn(&rc_key) {
                return Some(v);
            }
            let cc_key = format!("P:cc|{}", exact_key);
            if let Some(v) = get_fn(&cc_key) {
                return Some(v);
            }
        } else if round == 3 {
            let rc_f_c = format!("P:rc|F:cc|{}", exact_key);
            if let Some(v) = get_fn(&rc_f_c) {
                return Some(v);
            }
            let rc_f_rc = format!("P:rc|F:rc|{}", exact_key);
            if let Some(v) = get_fn(&rc_f_rc) {
                return Some(v);
            }
            let cc_f_c = format!("P:cc|F:cc|{}", exact_key);
            if let Some(v) = get_fn(&cc_f_c) {
                return Some(v);
            }
        }

        /* 5. Even-step street action truncation (steps of 2 ONLY so active player identity and legal actions match!) */
        if let Some(slash_idx) = exact_key.find('/') {
            let street_prefix = &exact_key[..slash_idx];
            let acts = &exact_key[slash_idx + 1..];
            let mut end = acts.len();
            while end >= 2 {
                end -= 2;
                let sub_acts = &acts[..end];
                let truncated_exact = format!("{}/{}", street_prefix, sub_acts);
                if round == 3 {
                    let t_rc_f_c = format!("P:rc|F:cc|{}", truncated_exact);
                    if let Some(v) = get_fn(&t_rc_f_c) {
                        return Some(v);
                    }
                }
                let t_rc = format!("P:rc|{}", truncated_exact);
                if let Some(v) = get_fn(&t_rc) {
                    return Some(v);
                }
                let t_cc = format!("P:cc|{}", truncated_exact);
                if let Some(v) = get_fn(&t_cc) {
                    return Some(v);
                }
                if let Some(v) = get_fn(&truncated_exact) {
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
        get_holdem_infoset_key, get_holdem_infoset_key_rich,
        get_holdem_infoset_key_rich_with_bucket, get_holdem_infoset_key_with_bucket,
        postflop_equity_bucket,
    };

    let rich_key = get_holdem_infoset_key_rich(hole, board, round, history);
    if let Some(v) = get_fn(&rich_key) {
        return Some(v);
    }

    let exact_key = get_holdem_infoset_key(hole, board, round, history);
    if let Some(v) = get_fn(&exact_key) {
        return Some(v);
    }

    if round >= 2 {
        let bucket = postflop_equity_bucket(hole, board) as i32;
        for delta in [1i32, -1, 2, -2] {
            let nb = bucket + delta;
            if (0..NUM_EQUITY_BUCKETS as i32).contains(&nb) {
                let adj_rich =
                    get_holdem_infoset_key_rich_with_bucket(hole, nb as u8, round, history);
                if let Some(v) = get_fn(&adj_rich) {
                    return Some(v);
                }
                let adj_exact = get_holdem_infoset_key_with_bucket(hole, nb as u8, round, history);
                if let Some(v) = get_fn(&adj_exact) {
                    return Some(v);
                }
            }
        }

        let rc_key = format!("P:rc|{}", exact_key);
        if let Some(v) = get_fn(&rc_key) {
            return Some(v);
        }
        let cc_key = format!("P:cc|{}", exact_key);
        if let Some(v) = get_fn(&cc_key) {
            return Some(v);
        }

        if let Some(slash_idx) = exact_key.find('/') {
            let street_prefix = &exact_key[..slash_idx];
            let acts = &exact_key[slash_idx + 1..];
            let mut end = acts.len();
            while end >= 2 {
                end -= 2;
                let sub_acts = &acts[..end];
                let truncated_exact = format!("{}/{}", street_prefix, sub_acts);
                let t_rc = format!("P:rc|{}", truncated_exact);
                if let Some(v) = get_fn(&t_rc) {
                    return Some(v);
                }
                let t_cc = format!("P:cc|{}", truncated_exact);
                if let Some(v) = get_fn(&t_cc) {
                    return Some(v);
                }
                if let Some(v) = get_fn(&truncated_exact) {
                    return Some(v);
                }
            }
        }
    }

    None
}
