/*
 * Fast O(1) Bitwise 7-Card Poker Hand Evaluator (Zero-allocation, no sorting, pure bitmasks).
 */

use super::types::Card;

#[inline]
pub fn evaluate_7cards(hole: &[Card; 2], board: &[Card]) -> u64 {
    let board_n = board.len().min(5);
    let total_cards = 2 + board_n;

    let mut rank_counts = [0u8; 13];
    let mut suit_counts = [0u8; 4];
    let mut rank_mask = 0u16;
    let mut suit_masks = [0u16; 4];

    /* Ingest hole cards */
    for c in hole.iter() {
        let r = c.rank as usize;
        let s = c.suit as usize;
        rank_counts[r] += 1;
        suit_counts[s] += 1;
        rank_mask |= 1 << r;
        suit_masks[s] |= 1 << r;
    }

    /* Ingest board cards */
    for c in &board[..board_n] {
        let r = c.rank as usize;
        let s = c.suit as usize;
        rank_counts[r] += 1;
        suit_counts[s] += 1;
        rank_mask |= 1 << r;
        suit_masks[s] |= 1 << r;
    }

    /* 1. Check Flush & Straight Flush */
    let flush_suit = suit_counts.iter().position(|&cnt| cnt >= 5);

    if let Some(s) = flush_suit {
        let smask = suit_masks[s];
        let st_mask = smask & (smask >> 1) & (smask >> 2) & (smask >> 3) & (smask >> 4);
        let mut sf_high = None;
        if st_mask != 0 {
            let top_bit = 15 - st_mask.leading_zeros() as u64;
            sf_high = Some(top_bit + 4);
        } else if (smask & 0x100F) == 0x100F {
            sf_high = Some(3); /* A-2-3-4-5 wheel straight flush */
        }

        if let Some(st_h) = sf_high {
            return (8 << 32) | st_h;
        }
    }

    /* 2. Collect Quads, Trips, and Pairs */
    let mut quads = None;
    let mut trips = [0u64; 2];
    let mut trips_len = 0;
    let mut pairs = [0u64; 3];
    let mut pairs_len = 0;

    for r in (0..13).rev() {
        match rank_counts[r] {
            4 => {
                if quads.is_none() {
                    quads = Some(r as u64);
                }
            }
            3 => {
                if trips_len < 2 {
                    trips[trips_len] = r as u64;
                    trips_len += 1;
                }
            }
            2 if pairs_len < 3 => {
                pairs[pairs_len] = r as u64;
                pairs_len += 1;
            }
            _ => {}
        }
    }

    /* 3. Four of a Kind */
    if let Some(q) = quads {
        let rem_mask = rank_mask & !(1 << q);
        let kicker = if rem_mask != 0 {
            15 - rem_mask.leading_zeros() as u64
        } else {
            0
        };
        return (7 << 32) | (q << 16) | kicker;
    }

    /* 4. Full House */
    if trips_len > 0 && (pairs_len > 0 || trips_len > 1) {
        let t = trips[0];
        let p = if trips_len > 1 { trips[1] } else { pairs[0] };
        return (6 << 32) | (t << 16) | p;
    }

    /* 5. Flush */
    if let Some(s) = flush_suit {
        let mut score = 5u64 << 32;
        let mut m = suit_masks[s];
        for i in 0..5 {
            let r = 15 - m.leading_zeros() as u64;
            score |= r << (16 - i * 4);
            m &= !(1 << r);
        }
        return score;
    }

    /* 6. Straight */
    let st_mask =
        rank_mask & (rank_mask >> 1) & (rank_mask >> 2) & (rank_mask >> 3) & (rank_mask >> 4);
    let mut straight_high = None;
    if st_mask != 0 {
        let top_bit = 15 - st_mask.leading_zeros() as u64;
        straight_high = Some(top_bit + 4);
    } else if (rank_mask & 0x100F) == 0x100F {
        straight_high = Some(3); /* A-2-3-4-5 wheel straight */
    }

    if let Some(st_h) = straight_high {
        return (4 << 32) | st_h;
    }

    /* 7. Three of a Kind */
    if trips_len > 0 {
        let t = trips[0];
        let mut rem_mask = rank_mask & !(1 << t);
        let k0 = if rem_mask != 0 {
            let r = 15 - rem_mask.leading_zeros() as u64;
            rem_mask &= !(1 << r);
            r
        } else {
            0
        };
        let k1 = if rem_mask != 0 {
            15 - rem_mask.leading_zeros() as u64
        } else {
            0
        };
        return (3 << 32) | (t << 16) | (k0 << 8) | k1;
    }

    /* 8. Two Pair */
    if pairs_len >= 2 {
        let p1 = pairs[0];
        let p2 = pairs[1];
        let rem_mask = rank_mask & !((1 << p1) | (1 << p2));
        let kicker = if rem_mask != 0 {
            15 - rem_mask.leading_zeros() as u64
        } else {
            0
        };
        return (2 << 32) | (p1 << 16) | (p2 << 8) | kicker;
    }

    /* 9. One Pair */
    if pairs_len >= 1 {
        let p = pairs[0];
        let mut rem_mask = rank_mask & !(1 << p);
        let k0 = if rem_mask != 0 {
            let r = 15 - rem_mask.leading_zeros() as u64;
            rem_mask &= !(1 << r);
            r
        } else {
            0
        };
        let k1 = if rem_mask != 0 {
            15 - rem_mask.leading_zeros() as u64
        } else {
            0
        };
        return (1 << 32) | (p << 16) | (k0 << 8) | k1;
    }

    /* 10. High Card */
    let mut score = 0u64;
    let mut rem_mask = rank_mask;
    let count = total_cards.min(5);
    for i in 0..count {
        if rem_mask == 0 {
            break;
        }
        let r = 15 - rem_mask.leading_zeros() as u64;
        score |= r << (16 - i * 4);
        rem_mask &= !(1 << r);
    }
    score
}
