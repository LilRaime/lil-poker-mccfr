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
