/*!
 * Fast O(1) Bitwise 7-Card Poker Hand Evaluator (Zero-allocation, vertical rank bitboard adder,
 * SWAR suit detection, and 8 KB L1 straight lookup table).
 */

use super::types::Card;
use crate::game::config::{NUM_COMMUNITY_CARDS, NUM_HOLE_CARDS, NUM_SUITS};

const fn build_straight_lut() -> [u8; 8192] {
    let mut lut = [0u8; 8192];
    let mut m = 0usize;
    while m < 8192 {
        let st = m & (m >> 1) & (m >> 2) & (m >> 3) & (m >> 4);
        if st != 0 {
            let top_bit = (15 - (st as u16).leading_zeros()) as u8;
            lut[m] = (top_bit + 4) + 1;
        } else if (m & 0x100F) == 0x100F {
            lut[m] = 3 + 1;
        }
        m += 1;
    }
    lut
}

static STRAIGHT_HIGH_LUT: [u8; 8192] = build_straight_lut();

#[inline]
pub fn evaluate_7cards(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> u64 {
    let board_n = board.len().min(NUM_COMMUNITY_CARDS);
    let total_cards = NUM_HOLE_CARDS + board_n;

    let mut m1 = 0u16;
    let mut m2 = 0u16;
    let mut m3 = 0u16;
    let mut m4 = 0u16;
    let mut suit_counts_packed = 0u32;
    let mut suit_masks = [0u16; NUM_SUITS];

    for c in hole {
        let r = c.rank as usize;
        let s = c.suit as usize;
        let bit = 1u16 << r;
        m4 |= m3 & bit;
        m3 |= m2 & bit;
        m2 |= m1 & bit;
        m1 |= bit;
        suit_counts_packed += 1u32 << (s * 8);
        suit_masks[s] |= bit;
    }

    for c in &board[..board_n] {
        let r = c.rank as usize;
        let s = c.suit as usize;
        let bit = 1u16 << r;
        m4 |= m3 & bit;
        m3 |= m2 & bit;
        m2 |= m1 & bit;
        m1 |= bit;
        suit_counts_packed += 1u32 << (s * 8);
        suit_masks[s] |= bit;
    }

    /* 1. SWAR Check for 5+ of any suit: (cnt + 3) sets bit 3 (0x08) iff cnt >= 5 (for cnt <= 7) */
    let flush_bits = (suit_counts_packed + 0x0303_0303) & 0x0808_0808;
    if flush_bits != 0 {
        let s = (flush_bits.trailing_zeros() >> 3) as usize;
        let sf = STRAIGHT_HIGH_LUT[suit_masks[s] as usize];
        if sf != 0 {
            return (8 << 32) | u64::from(sf - 1);
        }
    }

    /* 2. Four of a Kind */
    if m4 != 0 {
        let q = u64::from(15 - m4.leading_zeros());
        let rem_mask = m1 & !(1 << q);
        let kicker = if rem_mask != 0 {
            u64::from(15 - rem_mask.leading_zeros())
        } else {
            0
        };
        return (7 << 32) | (q << 16) | kicker;
    }

    /* 3. Full House */
    if m3 != 0 {
        let t = u64::from(15 - m3.leading_zeros());
        let other_pairs = m2 & !(1 << t);
        if other_pairs != 0 {
            let second_trip = m3 & !(1 << t);
            let p_mask = if second_trip != 0 {
                second_trip
            } else {
                other_pairs
            };
            let p = u64::from(15 - p_mask.leading_zeros());
            return (6 << 32) | (t << 16) | p;
        }
    }

    /* 4. Flush */
    if flush_bits != 0 {
        let s = (flush_bits.trailing_zeros() >> 3) as usize;
        let mut score = 5u64 << 32;
        let mut m = suit_masks[s];
        for i in 0..NUM_COMMUNITY_CARDS {
            if m == 0 {
                break;
            }
            let r = u64::from(15 - m.leading_zeros());
            score |= r << (16 - i * 4);
            m &= !(1 << r);
        }
        return score;
    }

    /* 5. Straight */
    let st = STRAIGHT_HIGH_LUT[m1 as usize];
    if st != 0 {
        return (4 << 32) | u64::from(st - 1);
    }

    /* 6. Three of a Kind */
    if m3 != 0 {
        let t = u64::from(15 - m3.leading_zeros());
        let mut rem_mask = m1 & !(1 << t);
        let k0 = if rem_mask != 0 {
            let r = u64::from(15 - rem_mask.leading_zeros());
            rem_mask &= !(1 << r);
            r
        } else {
            0
        };
        let k1 = if rem_mask != 0 {
            u64::from(15 - rem_mask.leading_zeros())
        } else {
            0
        };
        return (3 << 32) | (t << 16) | (k0 << 8) | k1;
    }

    /* 7. Two Pair & One Pair */
    if m2 != 0 {
        let p1 = u64::from(15 - m2.leading_zeros());
        let m2_rem = m2 & !(1 << p1);
        if m2_rem != 0 {
            let p2 = u64::from(15 - m2_rem.leading_zeros());
            let rem_mask = m1 & !((1 << p1) | (1 << p2));
            let kicker = if rem_mask != 0 {
                u64::from(15 - rem_mask.leading_zeros())
            } else {
                0
            };
            return (2 << 32) | (p1 << 16) | (p2 << 8) | kicker;
        }

        let mut rem_mask = m1 & !(1 << p1);
        let k0 = if rem_mask != 0 {
            let r = u64::from(15 - rem_mask.leading_zeros());
            rem_mask &= !(1 << r);
            r
        } else {
            0
        };
        let k1 = if rem_mask != 0 {
            let r = u64::from(15 - rem_mask.leading_zeros());
            rem_mask &= !(1 << r);
            r
        } else {
            0
        };
        let k2 = if rem_mask != 0 {
            u64::from(15 - rem_mask.leading_zeros())
        } else {
            0
        };
        return (1 << 32) | (p1 << 16) | (k0 << 12) | (k1 << 8) | (k2 << 4);
    }

    /* 8. High Card */
    let mut score = 0u64;
    let mut rem_mask = m1;
    let count = total_cards.min(NUM_COMMUNITY_CARDS);
    for i in 0..count {
        if rem_mask == 0 {
            break;
        }
        let r = u64::from(15 - rem_mask.leading_zeros());
        score |= r << (16 - i * 4);
        rem_mask &= !(1 << r);
    }
    score
}
