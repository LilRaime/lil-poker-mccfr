/*
 * Intelligent GTO-aligned Strategy Fallback and Equity Heuristic Engine.
 * Used when an Information Set key is not present in the offline strategy model.
 * Replaces naive uniform random play with mathematically sound poker logic.
 */

use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};
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
) -> [f64; 6] {
    let raw = if round == 1 {
        let (pf_idx, _) = preflop_bucket(hole[0], hole[1]);
        preflop_fallback(pf_idx, to_call)
    } else {
        let bucket = postflop_equity_bucket(hole, board);
        postflop_fallback(bucket, round, to_call, pot)
    };

    /* Zero out illegal actions and normalize */
    let mut filtered = [0.0f64; 6];
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

/* Preflop heuristic based on Sklansky-Chubukov / canonical 169 hand tiers */
fn preflop_fallback(idx: usize, to_call: i32) -> [f64; 6] {
    let mut s = [0.0f64; 6];

    if idx <= 15 {
        /* Tier 1: Premium (AA, KK, QQ, JJ, TT, 99, AKs, AQs, AJs, AKo, AQo) */
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
    } else if idx <= 45 {
        /* Tier 2: Strong (88, 77, 66, ATs-A8s, KQs-KTs, QJs, QTs, JTs, AJo, KQo) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.20;
            s[RAISE_MIN as usize] = 0.25;
            s[RAISE_THIRD_POT as usize] = 0.35;
            s[RAISE_HALF_POT as usize] = 0.20;
        } else if to_call <= PF_TIER2_CALL_MAX {
            s[CALL_CHECK as usize] = 0.65;
            s[RAISE_MIN as usize] = 0.15;
            s[RAISE_THIRD_POT as usize] = 0.15;
            s[RAISE_HALF_POT as usize] = 0.05;
        } else {
            s[FOLD as usize] = 0.25;
            s[CALL_CHECK as usize] = 0.60;
            s[RAISE_HALF_POT as usize] = 0.10;
            s[ALL_IN as usize] = 0.05;
        }
    } else if idx <= 85 {
        /* Tier 3: Playable / Speculative (55-22, suited connectors, suited Aces, broadways) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.75;
            s[RAISE_THIRD_POT as usize] = 0.15;
            s[RAISE_MIN as usize] = 0.10;
        } else if to_call <= PF_TIER3_CHEAP_CALL_MAX {
            s[CALL_CHECK as usize] = 0.80;
            s[RAISE_THIRD_POT as usize] = 0.10;
            s[FOLD as usize] = 0.10;
        } else if to_call <= PF_TIER3_FOLD_BOUNDARY {
            s[CALL_CHECK as usize] = 0.45;
            s[FOLD as usize] = 0.55;
        } else {
            s[FOLD as usize] = 0.90;
            s[CALL_CHECK as usize] = 0.10;
        }
    } else if idx <= 125 {
        /* Tier 4: Marginal (unsuited broadways, weak suited, low offsuit connectors) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.95;
            s[RAISE_MIN as usize] = 0.05;
        } else if to_call <= PF_TIER4_CALL_MAX {
            s[CALL_CHECK as usize] = 0.35;
            s[FOLD as usize] = 0.65;
        } else {
            s[FOLD as usize] = 0.96;
            s[RAISE_THIRD_POT as usize] = 0.04;
        }
    } else {
        /* Tier 5: Trash (unsuited rag hands: 72o, 83o, etc.) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 1.0;
        } else {
            s[FOLD as usize] = 0.98;
            s[RAISE_THIRD_POT as usize] = 0.02;
        }
    }

    s
}

/* Postflop heuristic based on 77 equity buckets, board round, pot odds, and aggression */
fn postflop_fallback(bucket: usize, round: u8, to_call: i32, pot: i32) -> [f64; 6] {
    let mut s = [0.0f64; 6];
    let total_pot = (pot + to_call).max(1);
    let pot_odds = to_call as f64 / total_pot as f64;

    if bucket >= 62 {
        /* Monsters / Nuts (Straight, Flush, Full House, Quads, Straight Flush) */
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
    } else if bucket >= 51 {
        /* Very Strong (Trips / Sets, High Two Pair) */
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
    } else if bucket >= 36 {
        /* Strong (Two Pair, Top Pair with Top Kicker) */
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
        /* Marginal / Bluff Catcher (One Pair: middle pair, bottom pair, weak kicker) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.80;
            s[RAISE_THIRD_POT as usize] = 0.15; /* Probe bet */
            s[RAISE_MIN as usize] = 0.05;
        } else if pot_odds <= 0.20 {
            /* Cheap call / good pot odds */
            s[CALL_CHECK as usize] = 0.70;
            s[FOLD as usize] = 0.30;
        } else if pot_odds <= 0.30 {
            s[CALL_CHECK as usize] = 0.45;
            s[FOLD as usize] = 0.55;
        } else {
            s[CALL_CHECK as usize] = 0.20;
            s[FOLD as usize] = 0.80;
        }
    } else if round < 4 && bucket >= 15 {
        /* Strong Draws on Flop & Turn (Flush draw, Open-ended straight draw, combo draw) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.40;
            s[RAISE_THIRD_POT as usize] = 0.40; /* Standard 33% semi-bluff c-bet! */
            s[RAISE_HALF_POT as usize] = 0.15;
            s[ALL_IN as usize] = 0.05; /* High pressure semi-bluff shove */
        } else if pot_odds <= 0.28 {
            s[CALL_CHECK as usize] = 0.75;
            s[RAISE_THIRD_POT as usize] = 0.15;
            s[FOLD as usize] = 0.10;
        } else {
            s[CALL_CHECK as usize] = 0.50;
            s[FOLD as usize] = 0.45;
            s[RAISE_THIRD_POT as usize] = 0.05;
        }
    } else if round < 4 && bucket >= 11 {
        /* Weak Draws on Flop & Turn (Gutshots) */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.80;
            s[RAISE_THIRD_POT as usize] = 0.20;
        } else if pot_odds <= 0.18 {
            s[CALL_CHECK as usize] = 0.65;
            s[FOLD as usize] = 0.35;
        } else {
            s[FOLD as usize] = 0.85;
            s[CALL_CHECK as usize] = 0.15;
        }
    } else {
        /* Weak / Air / Missed draw */
        if to_call == 0 {
            s[CALL_CHECK as usize] = 0.88;
            s[RAISE_THIRD_POT as usize] = 0.08; /* Small c-bet bluff */
            s[RAISE_HALF_POT as usize] = 0.04;
        } else {
            s[FOLD as usize] = 0.94;
            s[RAISE_THIRD_POT as usize] = 0.04;
            s[ALL_IN as usize] = 0.02; /* Polarized river shove bluff */
        }
    }

    s
}
