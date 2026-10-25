/*
 * Fast board texture analysis (Monotone, TwoTone, Rainbow, Paired, Connected).
 */

use crate::game::holdem::Card;

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

pub fn detect_board_texture(board: &[Card]) -> BoardTextureInfo {
    if board.is_empty() {
        return BoardTextureInfo {
            flush_texture: BoardFlushTexture::Rainbow,
            is_paired: false,
            is_connected: false,
        };
    }
    let mut suit_counts = [0u8; 4];
    let mut rank_counts = [0u8; 13];
    for &c in board {
        suit_counts[c.suit as usize] += 1;
        rank_counts[c.rank as usize] += 1;
    }
    let max_suit = *suit_counts.iter().max().unwrap_or(&0);
    let flush_texture = if max_suit >= 3 {
        BoardFlushTexture::Monotone
    } else if max_suit == 2 {
        BoardFlushTexture::TwoTone
    } else {
        BoardFlushTexture::Rainbow
    };

    let is_paired = rank_counts.iter().any(|&cnt| cnt >= 2);

    let mut is_connected = false;
    let mut consecutive = 0;
    for &cnt in &rank_counts {
        if cnt > 0 {
            consecutive += 1;
            if consecutive >= 2 {
                is_connected = true;
                break;
            }
        } else {
            consecutive = 0;
        }
    }
    if !is_connected && rank_counts[12] > 0 && (rank_counts[0] > 0 || rank_counts[11] > 0) {
        is_connected = true;
    }

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
    let paired_idx = if tex.is_paired { 1u8 } else { 0u8 };
    (flush_idx << 1) | paired_idx
}

/* Detects if community cards form a 3-flush or 4-flush board (3+ cards of the same suit). */
#[inline(always)]
pub fn is_three_flush_board(board: &[Card]) -> bool {
    if board.len() < 3 {
        return false;
    }
    let mut suit_counts = [0u8; 4];
    for &c in board {
        suit_counts[c.suit as usize] += 1;
    }
    suit_counts.iter().any(|&cnt| cnt >= 3)
}

/* Detects if community cards form a 4-flush board (4+ cards of the same suit). */
#[inline(always)]
pub fn is_four_flush_board(board: &[Card]) -> bool {
    if board.len() < 4 {
        return false;
    }
    let mut suit_counts = [0u8; 4];
    for &c in board {
        suit_counts[c.suit as usize] += 1;
    }
    suit_counts.iter().any(|&cnt| cnt >= 4)
}

/* Detects if community cards contain 4 or more cards to a straight (e.g. 9-T-J-Q or 5-6-7-8). */
#[inline(always)]
pub fn is_four_straight_board(board: &[Card]) -> bool {
    if board.len() < 4 {
        return false;
    }
    let mut rank_counts = [false; 13];
    for &c in board {
        rank_counts[c.rank as usize] = true;
    }
    for start in 0..=8 {
        let count = (start..start + 5).filter(|&r| rank_counts[r]).count();
        if count >= 4 {
            return true;
        }
    }
    let ace_low = [12, 0, 1, 2, 3];
    if ace_low.iter().filter(|&&r| rank_counts[r]).count() >= 4 {
        return true;
    }
    false
}

/* Detects if community cards form a dangerous, highly coordinated board
 * (e.g. 3+ cards to a flush, 4 cards to a straight, or coordinated 3-to-straight on turn/river).
 */
pub fn is_dangerous_board(board: &[Card]) -> bool {
    if board.len() < 3 {
        return false;
    }
    let mut suit_counts = [0u8; 4];
    let mut rank_counts = [false; 13];
    for &c in board {
        suit_counts[c.suit as usize] += 1;
        rank_counts[c.rank as usize] = true;
    }
    /* 3 or more to a flush (any 2 suited cards or 1 card on 4-flush completes a flush) */
    if suit_counts.iter().any(|&cnt| cnt >= 3) {
        return true;
    }
    /* 4 or more to a straight (4 ranks in any 5-card span) */
    for start in 0..=8 {
        let count = (start..start + 5).filter(|&r| rank_counts[r]).count();
        if count >= 4 {
            return true;
        }
    }
    /* Ace-low straight span (A, 2, 3, 4, 5) */
    let ace_low = [12, 0, 1, 2, 3];
    if ace_low.iter().filter(|&&r| rank_counts[r]).count() >= 4 {
        return true;
    }
    /* Coordinated paired board: paired board that also has 3 ranks within a 4-card straight span
     * (e.g. 5c 8s 4h 3h 4s in Match 2 where 3-4-5 + paired 4s creates high trips/straight danger) */
    if board.len() >= 4 {
        let mut r_freq = [0u8; 13];
        for &c in board {
            r_freq[c.rank as usize] += 1;
        }
        if r_freq.iter().any(|&cnt| cnt >= 2) {
            for start in 0..=9 {
                let count = (start..start + 4).filter(|&r| rank_counts[r]).count();
                if count >= 3 {
                    return true;
                }
            }
            let ace_low_4 = [12, 0, 1, 2];
            if ace_low_4.iter().filter(|&&r| rank_counts[r]).count() >= 3 {
                return true;
            }
        }
    }
    false
}
