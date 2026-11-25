/*!
 * Fast O(1) board texture analysis (`Monotone`, `TwoTone`, `Rainbow`, Paired, Connected)
 * using SWAR suit counters and an 8 KB L1 straight-span lookup table.
 */

use crate::game::holdem::Card;

const FLAG_CONNECTED: u8 = 0x01;
const FLAG_FOUR_STRAIGHT: u8 = 0x02;
const FLAG_THREE_IN_FOUR: u8 = 0x04;

const fn build_board_span_lut() -> [u8; 8192] {
    let mut lut = [0u8; 8192];
    let mut m = 0usize;
    while m < 8192 {
        let mut flags = 0u8;
        if (m & (m >> 1)) != 0 || (m & 0x1001) == 0x1001 {
            flags |= FLAG_CONNECTED;
        }
        let mut s5 = 0usize;
        while s5 <= 8 {
            if ((m >> s5) & 0x1F).count_ones() >= 4 {
                flags |= FLAG_FOUR_STRAIGHT;
                break;
            }
            s5 += 1;
        }
        if (m & 0x100F).count_ones() >= 4 {
            flags |= FLAG_FOUR_STRAIGHT;
        }
        let mut s4 = 0usize;
        while s4 <= 9 {
            if ((m >> s4) & 0x0F).count_ones() >= 3 {
                flags |= FLAG_THREE_IN_FOUR;
                break;
            }
            s4 += 1;
        }
        if (m & 0x1007).count_ones() >= 3 {
            flags |= FLAG_THREE_IN_FOUR;
        }
        lut[m] = flags;
        m += 1;
    }
    lut
}

static BOARD_SPAN_LUT: [u8; 8192] = build_board_span_lut();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardFlushTexture {
    Rainbow,
    TwoTone,
    Monotone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardTextureInfo {
    pub flush_texture: BoardFlushTexture,
    pub is_paired: bool,
    pub is_connected: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct FastBoardSummary {
    pub(crate) board_len: usize,
    pub(crate) is_four_fl: bool,
    pub(crate) is_four_st: bool,
    pub(crate) is_three_fl: bool,
    pub(crate) is_dangerous: bool,
    pub(crate) is_paired: bool,
    pub(crate) max_rank: usize,
    pub(crate) min_rank: usize,
}

#[inline(always)]
pub(crate) fn compute_board_summary(board: &[Card]) -> FastBoardSummary {
    let n = board.len();
    if n == 0 {
        return FastBoardSummary {
            board_len: 0,
            is_four_fl: false,
            is_four_st: false,
            is_three_fl: false,
            is_dangerous: false,
            is_paired: false,
            max_rank: 0,
            min_rank: 0,
        };
    }

    let mut m1 = 0u16;
    let mut m2 = 0u16;
    let mut suit_packed = 0u32;
    for &c in board {
        let bit = 1u16 << (c.rank as usize);
        m2 |= m1 & bit;
        m1 |= bit;
        suit_packed += 1u32 << ((c.suit as usize) * 8);
    }

    let is_paired = m2 != 0;
    let is_three_fl = n >= 3 && ((suit_packed + 0x0505_0505) & 0x0808_0808) != 0;
    let is_four_fl = n >= 4 && ((suit_packed + 0x0404_0404) & 0x0808_0808) != 0;
    let span_flags = BOARD_SPAN_LUT[m1 as usize];
    let is_four_st = n >= 4 && (span_flags & FLAG_FOUR_STRAIGHT) != 0;
    let is_dangerous = n >= 3
        && (is_three_fl
            || (span_flags & FLAG_FOUR_STRAIGHT) != 0
            || (n >= 4 && is_paired && (span_flags & FLAG_THREE_IN_FOUR) != 0));
    let max_rank = (15 - m1.leading_zeros()) as usize;
    let min_rank = m1.trailing_zeros() as usize;

    FastBoardSummary {
        board_len: n,
        is_four_fl,
        is_four_st,
        is_three_fl,
        is_dangerous,
        is_paired,
        max_rank,
        min_rank,
    }
}

#[inline]
pub fn detect_board_texture(board: &[Card]) -> BoardTextureInfo {
    if board.is_empty() {
        return BoardTextureInfo {
            flush_texture: BoardFlushTexture::Rainbow,
            is_paired: false,
            is_connected: false,
        };
    }

    let mut m1 = 0u16;
    let mut m2 = 0u16;
    let mut suit_packed = 0u32;
    for &c in board {
        let bit = 1u16 << (c.rank as usize);
        m2 |= m1 & bit;
        m1 |= bit;
        suit_packed += 1u32 << ((c.suit as usize) * 8);
    }

    let flush_texture = if ((suit_packed + 0x0505_0505) & 0x0808_0808) != 0 {
        BoardFlushTexture::Monotone
    } else if ((suit_packed + 0x0606_0606) & 0x0808_0808) != 0 {
        BoardFlushTexture::TwoTone
    } else {
        BoardFlushTexture::Rainbow
    };
    let is_paired = m2 != 0;
    let is_connected = (BOARD_SPAN_LUT[m1 as usize] & FLAG_CONNECTED) != 0;

    BoardTextureInfo {
        flush_texture,
        is_paired,
        is_connected,
    }
}

#[inline(always)]
pub fn board_texture_code(board: &[Card]) -> &'static str {
    if board.is_empty() {
        return "R";
    }
    let tex = detect_board_texture(board);
    if tex.is_paired {
        match tex.flush_texture {
            BoardFlushTexture::Monotone => "MP",
            BoardFlushTexture::TwoTone => "TP",
            BoardFlushTexture::Rainbow => "RP",
        }
    } else {
        match tex.flush_texture {
            BoardFlushTexture::Monotone => "M",
            BoardFlushTexture::TwoTone => "T",
            BoardFlushTexture::Rainbow => "R",
        }
    }
}

/** Compact 4-state postflop board texture char for flop blueprint infoset keys:
 * 'm' = monotone (3+ same suit), 'p' = paired, 'w' = wet (two-tone & connected), 'd' = dry. */
#[inline(always)]
pub fn compact_board_texture_char(board: &[Card]) -> char {
    if board.is_empty() {
        return 'd';
    }
    let tex = detect_board_texture(board);
    if tex.flush_texture == BoardFlushTexture::Monotone {
        'm'
    } else if tex.is_paired {
        'p'
    } else if tex.flush_texture == BoardFlushTexture::TwoTone && tex.is_connected {
        'w'
    } else {
        'd'
    }
}

/** Compact 3-state turn board texture char for turn blueprint infoset keys (`T:`):
 * `'x'` = 3-flush, 4-flush, or 4-straight danger; `'p'` = paired; `'c'` = clean. */
#[inline(always)]
pub fn compact_turn_texture_char(board: &[Card]) -> char {
    if board.len() < 4 {
        return 'c';
    }
    let summary = compute_board_summary(board);
    if summary.is_three_fl || summary.is_four_fl || summary.is_four_st {
        'x'
    } else if summary.is_paired {
        'p'
    } else {
        'c'
    }
}

#[inline(always)]
pub fn board_texture_byte(board: &[Card]) -> u8 {
    if board.is_empty() {
        return 0;
    }
    let tex = detect_board_texture(board);
    let flush_idx = match tex.flush_texture {
        BoardFlushTexture::Rainbow => 0u8,
        BoardFlushTexture::TwoTone => 1u8,
        BoardFlushTexture::Monotone => 2u8,
    };
    let paired_idx = u8::from(tex.is_paired);
    (flush_idx << 1) | paired_idx
}

/** Detects if community cards form a 3-flush or 4-flush board (3+ cards of the same suit). */
#[inline(always)]
pub fn is_three_flush_board(board: &[Card]) -> bool {
    if board.len() < 3 {
        return false;
    }
    let mut suit_packed = 0u32;
    for &c in board {
        suit_packed += 1u32 << ((c.suit as usize) * 8);
    }
    ((suit_packed + 0x0505_0505) & 0x0808_0808) != 0
}

/** Detects if community cards form a 4-flush board (4+ cards of the same suit). */
#[inline(always)]
pub fn is_four_flush_board(board: &[Card]) -> bool {
    if board.len() < 4 {
        return false;
    }
    let mut suit_packed = 0u32;
    for &c in board {
        suit_packed += 1u32 << ((c.suit as usize) * 8);
    }
    ((suit_packed + 0x0404_0404) & 0x0808_0808) != 0
}

/** Detects if community cards contain 4 or more cards to a straight (e.g. 9-T-J-Q or 5-6-7-8). */
#[inline(always)]
pub fn is_four_straight_board(board: &[Card]) -> bool {
    if board.len() < 4 {
        return false;
    }
    let mut m1 = 0u16;
    for &c in board {
        m1 |= 1u16 << (c.rank as usize);
    }
    (BOARD_SPAN_LUT[m1 as usize] & FLAG_FOUR_STRAIGHT) != 0
}

/** Detects if community cards form a dangerous, highly coordinated board
 * (e.g. 3+ cards to a flush, 4 cards to a straight, or coordinated 3-to-straight on turn/river).
 */
#[inline]
pub fn is_dangerous_board(board: &[Card]) -> bool {
    if board.len() < 3 {
        return false;
    }
    compute_board_summary(board).is_dangerous
}
