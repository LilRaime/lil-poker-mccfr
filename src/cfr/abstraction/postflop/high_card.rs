/*! Flop/Turn Draws, Backdoor Draws, and River High-Card Bucketing */
use crate::game::config::NUM_HOLE_CARDS;
use crate::game::holdem::Card;

use super::super::draws::{detect_draws_detailed, DrawInfo};

pub(super) fn bucket_high_card_or_draw(hole: [Card; NUM_HOLE_CARDS], board: &[Card]) -> usize {
    let hole_high = (hole[0].rank as usize).max(hole[1].rank as usize);
    let hole_low = (hole[0].rank as usize).min(hole[1].rank as usize);
    if board.len() >= 5 {
        return bucket_unpaired_high_card(hole_high, hole_low, true);
    }

    let max_board_rank = board.iter().map(|c| c.rank as usize).max().unwrap_or(0);
    let overcards = (hole[0].rank as usize > max_board_rank) as usize
        + (hole[1].rank as usize > max_board_rank) as usize;
    let backdoors = if board.len() == 3 {
        detect_flop_backdoors(hole, board)
    } else {
        (false, false, false)
    };
    let draws = detect_draws_detailed(&hole, board);

    bucket_flop_turn_draws(draws, hole_high, overcards, backdoors)
        .unwrap_or_else(|| bucket_unpaired_high_card(hole_high, hole_low, false))
}

fn detect_flop_backdoors(hole: [Card; NUM_HOLE_CARDS], board: &[Card]) -> (bool, bool, bool) {
    let mut bdfd = false;
    let mut nut_bdfd = false;
    for s in 0..4u8 {
        let hole_suit_cnt = (hole[0].suit as u8 == s) as usize + (hole[1].suit as u8 == s) as usize;
        let board_suit_cnt = board.iter().filter(|c| c.suit as u8 == s).count();
        if hole_suit_cnt >= 1 && board_suit_cnt <= 2 && hole_suit_cnt + board_suit_cnt == 3 {
            bdfd = true;
            let has_ace = (hole[0].suit as u8 == s && hole[0].rank as usize == 12)
                || (hole[1].suit as u8 == s && hole[1].rank as usize == 12);
            let has_king_with_board_ace = ((hole[0].suit as u8 == s
                && hole[0].rank as usize == 11)
                || (hole[1].suit as u8 == s && hole[1].rank as usize == 11))
                && board
                    .iter()
                    .any(|c| c.suit as u8 == s && c.rank as usize == 12);
            if has_ace || has_king_with_board_ace {
                nut_bdfd = true;
            }
        }
    }

    let mut rank_mask = 0u16;
    let mut hole_mask = 0u16;
    for c in board {
        rank_mask |= 1 << (c.rank as usize);
    }
    for c in hole {
        let r = c.rank as usize;
        rank_mask |= 1 << r;
        hole_mask |= 1 << r;
    }
    let bdsd = (0..=8).any(|start| {
        let win = (rank_mask >> start) & 0x1F;
        let h_win = (hole_mask >> start) & 0x1F;
        win.count_ones() >= 3 && h_win != 0
    });
    (bdfd, nut_bdfd, bdsd)
}

fn bucket_flop_turn_draws(
    draws: DrawInfo,
    hole_high: usize,
    overcards: usize,
    (has_backdoor_fd, has_nut_backdoor_fd, has_backdoor_sd): (bool, bool, bool),
) -> Option<usize> {
    if draws.is_nut_flush_draw && draws.is_straight_draw {
        Some(27) /* Nut Flush Draw + OESD */
    } else if draws.is_nut_flush_draw && draws.is_gutshot {
        Some(26) /* Nut Flush Draw + Gutshot */
    } else if draws.is_flush_draw && draws.is_straight_draw {
        Some(25) /* Flush Draw + OESD */
    } else if draws.is_flush_draw && draws.is_gutshot {
        Some(24) /* Flush Draw + Gutshot */
    } else if draws.is_nut_flush_draw {
        Some(if hole_high == 12 { 23 } else { 22 })
    } else if draws.is_flush_draw {
        Some(if hole_high >= 10 {
            21 /* High Flush Draw */
        } else if hole_high >= 7 {
            20 /* Mid Flush Draw */
        } else {
            19 /* Low Flush Draw */
        })
    } else if draws.is_straight_draw {
        Some(if has_backdoor_fd && overcards >= 1 {
            18 /* OESD + Backdoor Flush Draw + Overcard */
        } else if overcards == 2 || hole_high >= 11 {
            17 /* OESD with Two Overcards / Broadway */
        } else if overcards == 1 || hole_high >= 9 {
            16 /* OESD with One Overcard */
        } else {
            15 /* Low/Mid OESD */
        })
    } else if draws.is_gutshot {
        Some(if has_backdoor_fd && overcards >= 1 {
            14 /* Gutshot + Backdoor Flush Draw + Overcard */
        } else if overcards == 2 || hole_high >= 11 {
            13 /* Gutshot with Two Overcards */
        } else if overcards == 1 || hole_high >= 9 {
            12 /* Gutshot with One Overcard */
        } else {
            11 /* Weak Gutshot */
        })
    } else if overcards == 2 && has_backdoor_fd && hole_high >= 11 {
        Some(14) /* Two Broadway Overcards + Backdoor Flush Draw */
    } else if has_backdoor_fd
        && (overcards == 2 || (has_nut_backdoor_fd && has_backdoor_sd && overcards >= 1))
    {
        Some(12) /* Flop Backdoor Flush Draw + 2 Overcards */
    } else if has_nut_backdoor_fd && (overcards >= 1 || has_backdoor_sd) {
        Some(11) /* Flop Nut Backdoor Flush Draw + Overcard / Backdoor SD */
    } else {
        None
    }
}

fn bucket_unpaired_high_card(hole_high: usize, hole_low: usize, is_river: bool) -> usize {
    match hole_high {
        12 => {
            if hole_low >= 9 {
                10
            } else {
                9
            }
        }
        11 => {
            if is_river {
                8
            } else {
                7
            }
        }
        10 => 6,
        9 => 5,
        8 => 4,
        7 => 3,
        6 => 2,
        5 => 1,
        _ => 0,
    }
}
