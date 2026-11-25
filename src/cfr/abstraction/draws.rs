/*!
 * Fast O(1) draw detection for Flop & Turn (Flush draws, Nut Flush draws, Straight draws, Gutshots)
 * backed by compile-time 16 KB rank-mask window lookup tables.
 */

use crate::game::config::{NUM_COMMUNITY_CARDS, NUM_HOLE_CARDS, NUM_SUITS};
use crate::game::holdem::{Card, Rank};

const fn build_oesd_lut() -> [u16; 8192] {
    let mut lut = [0u16; 8192];
    let mut m = 0usize;
    while m < 8192 {
        let mut wins = 0u16;
        let mut r = 0usize;
        while r < 9 {
            let w = 0x0Fusize << r;
            if (m & w) == w {
                wins |= 1u16 << r;
            }
            r += 1;
        }
        lut[m] = wins;
        m += 1;
    }
    lut
}

const fn build_gutshot_lut() -> [u16; 8192] {
    let mut lut = [0u16; 8192];
    let mut m = 0usize;
    while m < 8192 {
        let mut wins = 0u16;
        let mut r = 0usize;
        while r < 9 {
            if ((m >> r) & 0x1F).count_ones() == 4 {
                wins |= 1u16 << r;
            }
            r += 1;
        }
        if (m & 0x100F).count_ones() == 4 {
            wins |= 1u16 << 9;
        }
        lut[m] = wins;
        m += 1;
    }
    lut
}

static OESD_WIN_LUT: [u16; 8192] = build_oesd_lut();
static GUTSHOT_WIN_LUT: [u16; 8192] = build_gutshot_lut();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawInfo {
    pub is_flush_draw: bool,
    pub is_nut_flush_draw: bool,
    pub is_straight_draw: bool,
    pub is_gutshot: bool,
}

#[inline(always)]
pub fn detect_draws_detailed(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> DrawInfo {
    let board_len = board.len();
    if board_len >= NUM_COMMUNITY_CARDS {
        return DrawInfo {
            is_flush_draw: false,
            is_nut_flush_draw: false,
            is_straight_draw: false,
            is_gutshot: false,
        };
    }

    let mut board_suit_counts = [0u8; NUM_SUITS];
    let mut board_mask = 0u16;
    let mut ace_suit_mask = 0u8;
    for &b in board {
        let s = b.suit as usize;
        board_suit_counts[s] += 1;
        board_mask |= 1u16 << (b.rank as usize);
        if b.rank == Rank::Ace {
            ace_suit_mask |= 1u8 << s;
        }
    }

    let h0_r = hole[0].rank as usize;
    let h1_r = hole[1].rank as usize;
    let h0_s = hole[0].suit as usize;
    let h1_s = hole[1].suit as usize;

    let mut is_flush_draw = false;
    let mut is_nut_flush_draw = false;
    if h0_s == h1_s {
        if board_suit_counts[h0_s] == 2 {
            is_flush_draw = true;
            let max_r = h0_r.max(h1_r);
            let ace_on_b = (ace_suit_mask & (1u8 << h0_s)) != 0;
            if max_r == (Rank::Ace as usize) || (ace_on_b && max_r == (Rank::King as usize)) {
                is_nut_flush_draw = true;
            }
        }
    } else {
        for &(s, hr) in &[(h0_s, h0_r), (h1_s, h1_r)] {
            if board_suit_counts[s] == 3 {
                let ace_on_b = (ace_suit_mask & (1u8 << s)) != 0;
                let is_nut_card =
                    hr == (Rank::Ace as usize) || (ace_on_b && hr == (Rank::King as usize));
                if hr >= (Rank::King as usize) || (board_len == 3 && hr >= (Rank::Jack as usize)) {
                    is_flush_draw = true;
                    if is_nut_card {
                        is_nut_flush_draw = true;
                    }
                }
                break;
            }
        }
    }

    let hole_mask = (1u16 << h0_r) | (1u16 << h1_r);
    let hole_only = hole_mask & !board_mask;
    let ranks_mask = board_mask | hole_mask;

    let mut is_straight_draw = false;
    let mut oesd_w = OESD_WIN_LUT[ranks_mask as usize];
    while oesd_w != 0 {
        let r = oesd_w.trailing_zeros() as usize;
        if (hole_only & (0x0Fu16 << r)) != 0 {
            is_straight_draw = true;
            break;
        }
        oesd_w &= oesd_w - 1;
    }

    let mut is_gutshot = false;
    if !is_straight_draw {
        let mut gs_w = GUTSHOT_WIN_LUT[ranks_mask as usize];
        while gs_w != 0 {
            let r = gs_w.trailing_zeros() as usize;
            let win_mask = if r < 9 { 0x1Fu16 << r } else { 0x100Fu16 };
            if (hole_only & win_mask) != 0 {
                is_gutshot = true;
                break;
            }
            gs_w &= gs_w - 1;
        }
    }

    DrawInfo {
        is_flush_draw,
        is_nut_flush_draw,
        is_straight_draw,
        is_gutshot,
    }
}

/** Fast `O(1)` double-gutshot detector (2+ distinct gutshot windows without an OESD). */
#[inline(always)]
pub fn is_double_gutshot(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> bool {
    if board.len() >= NUM_COMMUNITY_CARDS || board.len() < 3 {
        return false;
    }
    let mut board_mask = 0u16;
    for &b in board {
        board_mask |= 1u16 << (b.rank as usize);
    }
    let hole_mask = (1u16 << (hole[0].rank as usize)) | (1u16 << (hole[1].rank as usize));
    let hole_only = hole_mask & !board_mask;
    if hole_only == 0 {
        return false;
    }
    let ranks_mask = board_mask | hole_mask;
    let mut oesd_w = OESD_WIN_LUT[ranks_mask as usize];
    while oesd_w != 0 {
        let r = oesd_w.trailing_zeros() as usize;
        if (hole_only & (0x0Fu16 << r)) != 0 {
            return false;
        }
        oesd_w &= oesd_w - 1;
    }
    let mut gs_count = 0u32;
    let mut gs_w = GUTSHOT_WIN_LUT[ranks_mask as usize];
    while gs_w != 0 {
        let r = gs_w.trailing_zeros() as usize;
        let win_mask = if r < 9 { 0x1Fu16 << r } else { 0x100Fu16 };
        if (hole_only & win_mask) != 0 {
            gs_count += 1;
            if gs_count >= 2 {
                return true;
            }
        }
        gs_w &= gs_w - 1;
    }
    false
}

/** Backward-compatible draw detection returning (`is_flush_draw`, `is_straight_draw`) */
#[inline(always)]
pub fn detect_draws(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> (bool, bool) {
    let d = detect_draws_detailed(hole, board);
    (d.is_flush_draw, d.is_straight_draw)
}

/** Fast blocker detection for River bluffing:
 * Checks whether the player holds a key blocker that removes the opponent's nut hands:
 * 1. Nut Flush Blocker: board has >= 3 cards of suit S, and player holds the Ace of suit S.
 * 2. Broadway Straight Blocker: board has >= 3 broadway cards, and player holds an Ace or King. */
#[inline]
pub fn detect_river_nut_blocker(hole: &[Card; NUM_HOLE_CARDS], board: &[Card]) -> bool {
    if board.len() < 5 {
        return false;
    }
    let mut board_suit_counts = [0u8; NUM_SUITS];
    for &b in board {
        board_suit_counts[b.suit as usize] += 1;
    }
    for (s, &cnt) in board_suit_counts.iter().enumerate() {
        if cnt >= 3
            && ((hole[0].rank == Rank::Ace && hole[0].suit as usize == s)
                || (hole[1].rank == Rank::Ace && hole[1].suit as usize == s))
        {
            return true;
        }
    }
    let broadway_count = board.iter().filter(|c| c.rank as usize >= 8).count();
    let ace_on_board = board.iter().any(|c| c.rank == Rank::Ace);
    broadway_count >= 3 && !ace_on_board && (hole[0].rank == Rank::Ace || hole[1].rank == Rank::Ace)
}
