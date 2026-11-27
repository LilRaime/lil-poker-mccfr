use super::{cards, hole};
use lil_poker_mccfr::cfr::abstraction::{
    BoardFlushTexture, DrawInfo, detect_board_texture, detect_draws, detect_draws_detailed,
    is_dangerous_board, is_four_flush_board, is_four_straight_board, is_three_flush_board,
    postflop_equity_bucket, postflop_equity_bucket_with_score,
};
use lil_poker_mccfr::game::holdem::{ALL_52_CARDS, Card, Rank, evaluate_7cards};
use rand::SeedableRng;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;

#[test]
fn test_evaluate_7cards_categories() {
    for (h, b, cat, low) in [
        ("Ah Kh", "Qh Jh Th 9c 2d", 8, None),
        ("9s 9h", "9d 9c Kc 2h 3d", 7, None),
        ("Ks Kh", "Kd 4c 4h 2d 8c", 6, None),
        ("As Ts", "8s 6s 2s Kh Qd", 5, None),
        ("As 2h", "3d 4c 5s Kh 8d", 4, Some(3)),
        ("Qs Qh", "Qd 9c 7h 4d 2c", 3, None),
        ("Js Jh", "8d 8c Ac 4d 2h", 2, None),
        ("Ts Th", "Ad 8c 6c 4d 2h", 1, None),
        ("As Kh", "Jd 8c 6c 4d 2h", 0, None),
    ] {
        let score = evaluate_7cards(&hole(h), &cards(b));
        assert_eq!(score >> 32, cat, "Expected category {cat} for {h} / {b}");
        if let Some(lo) = low {
            assert_eq!(score & 0xFF, lo, "Expected 5-high straight");
        }
    }
}

#[test]
fn test_player_draw_contribution() {
    assert!(!detect_draws(&hole("Ks 9d"), &cards("Ah 7h 5h 2h")).0);
    assert!(detect_draws(&hole("Kh 9d"), &cards("Ah 7h 5h")).0);
}

#[test]
fn test_bitwise_evaluate_7cards_tiebreakers() {
    /* 1. Straight: Broadway A-K-Q-J-T vs Wheel 5-4-3-2-A */
    let score_broadway = evaluate_7cards(&hole("As Kh"), &cards("Qd Jc Th 4s 2c"));
    let score_wheel = evaluate_7cards(&hole("As 2h"), &cards("3d 4c 5h 9s 8c"));
    assert_eq!((score_broadway >> 32, score_broadway & 0xFF), (4, 12));
    assert_eq!((score_wheel >> 32, score_wheel & 0xFF), (4, 3));
    assert!(score_broadway > score_wheel, "Broadway must beat Wheel");

    /* 2. Flush tiebreakers: A♠ K♠ Q♠ J♠ 9♠ vs A♠ K♠ Q♠ J♠ 8♠ */
    let board_fl = cards("Ks Qs Js 3d 2h");
    let score_fl1 = evaluate_7cards(&hole("As 9s"), &board_fl);
    let score_fl2 = evaluate_7cards(&hole("As 8s"), &board_fl);
    assert_eq!((score_fl1 >> 32, score_fl2 >> 32), (5, 5));
    assert!(score_fl1 > score_fl2, "Higher 5th flush card must win");

    /* 3. Full House: KKK22 vs QQQAA */
    let score_fh_k = evaluate_7cards(&hole("Ks Kh"), &cards("Kd 2c 2d 7h 8c"));
    let score_fh_q = evaluate_7cards(&hole("Qs Qh"), &cards("Qd Ac Ad 7h 8c"));
    assert_eq!((score_fh_k >> 32, score_fh_q >> 32), (6, 6));
    assert!(score_fh_k > score_fh_q, "KKK22 must beat QQQAA");

    /* 4. Three pairs on 7 cards (AA KK QQ): best 2 pairs (AA-KK) + Q kicker */
    let score_3p1 = evaluate_7cards(&hole("As Ks"), &cards("Ah Kh Qh Qd 2c"));
    assert_eq!(score_3p1 >> 32, 2, "Must be Two Pair category 2");
    assert_eq!((score_3p1 >> 16) & 0xFF, 12, "Top pair Ace (12)");
    assert_eq!((score_3p1 >> 8) & 0xFF, 11, "Second pair King (11)");
    assert_eq!(score_3p1 & 0xFF, 10, "Kicker Queen (10)");
}

#[test]
fn test_one_pair_third_kicker_tiebreaker() {
    let board = cards("As Kh Qd 2c 3c");
    let score1 = evaluate_7cards(&hole("Ac 9s"), &board);
    let score2 = evaluate_7cards(&hole("Ah 8s"), &board);
    assert_eq!((score1 >> 32, score2 >> 32), (1, 1));
    assert!(score1 > score2, "AA-KQ9 ({score1:x}) > AA-KQ8 ({score2:x})");
}

#[test]
fn test_dangerous_board_detection() {
    for (b, expected) in [
        ("4d 6d 7c 3c", true),
        ("Ah 9h 6h 2h Kc", true),
        ("Ks 8d 2c", false),
    ] {
        assert_eq!(is_dangerous_board(&cards(b)), expected, "board {b}");
    }
}

#[test]
fn test_wheel_and_broadway_gutshot_vs_open_ended_draws() {
    for (h, b, expect_gutshot) in [
        ("As 2h", "3c 4d Ks", true),
        ("Ac Jd", "Kh Qs 2c", true),
        ("6c 7d", "8h 9s 2c", false),
        ("2h 3d", "4c 5s Kh", false),
    ] {
        let d = detect_draws_detailed(&hole(h), &cards(b));
        if expect_gutshot {
            assert!(d.is_gutshot && !d.is_straight_draw, "{h} on {b} gutshot");
        } else {
            assert!(d.is_straight_draw, "{h} on {b} OESD");
        }
    }
}

fn reference_evaluate_7cards(hole: [Card; 2], board: &[Card]) -> u64 {
    fn pop_top(m: &mut u16) -> u64 {
        if *m == 0 {
            return 0;
        }
        let r = u64::from(15 - m.leading_zeros());
        *m &= !(1 << r);
        r
    }
    fn straight_high(mask: u16) -> Option<u64> {
        let st = mask & (mask >> 1) & (mask >> 2) & (mask >> 3) & (mask >> 4);
        if st != 0 {
            Some(u64::from(15 - st.leading_zeros()) + 4)
        } else if (mask & 0x100F) == 0x100F {
            Some(3)
        } else {
            None
        }
    }

    let board_n = board.len().min(5);
    let (mut rank_counts, mut suit_counts, mut rank_mask, mut suit_masks) =
        ([0u8; 13], [0u8; 4], 0u16, [0u16; 4]);
    for c in hole.iter().chain(board[..board_n].iter()) {
        let (r, s) = (c.rank as usize, c.suit as usize);
        rank_counts[r] += 1;
        suit_counts[s] += 1;
        rank_mask |= 1 << r;
        suit_masks[s] |= 1 << r;
    }

    let flush_suit = suit_counts.iter().position(|&cnt| cnt >= 5);
    if let Some(s) = flush_suit
        && let Some(hi) = straight_high(suit_masks[s])
    {
        return (8 << 32) | hi;
    }

    let (mut quads, mut trips, mut tlen, mut pairs, mut plen) =
        (None, [0u64; 2], 0usize, [0u64; 3], 0usize);
    for r in (0..13).rev() {
        if rank_counts[r] == 4 {
            quads = Some(r as u64);
        } else if rank_counts[r] == 3 {
            trips[tlen] = r as u64;
            tlen += 1;
        } else if rank_counts[r] == 2 {
            pairs[plen] = r as u64;
            plen += 1;
        }
    }

    if let Some(q) = quads {
        let mut rem = rank_mask & !(1 << q);
        return (7 << 32) | (q << 16) | pop_top(&mut rem);
    }
    if tlen >= 2 {
        return (6 << 32) | (trips[0] << 16) | trips[1];
    }
    if tlen == 1 && plen >= 1 {
        return (6 << 32) | (trips[0] << 16) | pairs[0];
    }
    if let Some(s) = flush_suit {
        let (mut score, mut m) = (5u64 << 32, suit_masks[s]);
        for i in 0..5 {
            score |= pop_top(&mut m) << (16 - i * 4);
        }
        return score;
    }
    if let Some(hi) = straight_high(rank_mask) {
        return (4 << 32) | hi;
    }
    if tlen > 0 {
        let mut rem = rank_mask & !(1 << trips[0]);
        let (k0, k1) = (pop_top(&mut rem), pop_top(&mut rem));
        return (3 << 32) | (trips[0] << 16) | (k0 << 8) | k1;
    }
    if plen >= 2 {
        let mut rem = rank_mask & !((1 << pairs[0]) | (1 << pairs[1]));
        return (2 << 32) | (pairs[0] << 16) | (pairs[1] << 8) | pop_top(&mut rem);
    }
    if plen >= 1 {
        let mut rem = rank_mask & !(1 << pairs[0]);
        let (k0, k1, k2) = (pop_top(&mut rem), pop_top(&mut rem), pop_top(&mut rem));
        return (1 << 32) | (pairs[0] << 16) | (k0 << 12) | (k1 << 8) | (k2 << 4);
    }
    let (mut score, mut rem) = (0u64, rank_mask);
    for i in 0..(2 + board_n).min(5) {
        score |= pop_top(&mut rem) << (16 - i * 4);
    }
    score
}

fn reference_detect_draws_detailed(hole: [Card; 2], board: &[Card]) -> DrawInfo {
    if board.len() >= 5 {
        return DrawInfo {
            is_flush_draw: false,
            is_nut_flush_draw: false,
            is_straight_draw: false,
            is_gutshot: false,
        };
    }

    let (mut board_suits, mut board_ranks, mut ranks) = ([0u8; 4], [false; 13], [false; 13]);
    for &b in board {
        board_suits[b.suit as usize] += 1;
        board_ranks[b.rank as usize] = true;
        ranks[b.rank as usize] = true;
    }
    let (mut is_flush_draw, mut is_nut_flush_draw) = (false, false);
    for (s, &b_count) in board_suits.iter().enumerate() {
        let hole_has = u8::from(hole[0].suit as usize == s) + u8::from(hole[1].suit as usize == s);
        if hole_has >= 1 && (b_count + hole_has == 4) {
            let max_r = hole
                .iter()
                .filter(|c| c.suit as usize == s)
                .map(|c| c.rank as usize)
                .max()
                .unwrap_or(0);
            let ace_board = board
                .iter()
                .any(|c| c.suit as usize == s && c.rank == Rank::Ace);
            if hole_has == 2
                || max_r >= Rank::King as usize
                || (board.len() == 3 && max_r >= Rank::Jack as usize)
            {
                is_flush_draw = true;
                is_nut_flush_draw =
                    max_r == Rank::Ace as usize || (ace_board && max_r == Rank::King as usize);
            }
            break;
        }
    }

    let (h0, h1) = (hole[0].rank as usize, hole[1].rank as usize);
    ranks[h0] = true;
    ranks[h1] = true;
    let hole_in = |lo: usize, hi: usize| {
        (h0 >= lo && h0 <= hi && !board_ranks[h0]) || (h1 >= lo && h1 <= hi && !board_ranks[h1])
    };
    let is_straight_draw = (0..9)
        .any(|r| ranks[r] && ranks[r + 1] && ranks[r + 2] && ranks[r + 3] && hole_in(r, r + 3));
    let is_gutshot = !is_straight_draw
        && ((0..9).any(|r| (r..r + 5).filter(|&i| ranks[i]).count() == 4 && hole_in(r, r + 4))
            || (ranks[12]
                && (0..4).filter(|&i| ranks[i]).count() == 3
                && (((h0 == 12 || h0 <= 3) && !board_ranks[h0])
                    || ((h1 == 12 || h1 <= 3) && !board_ranks[h1]))));

    DrawInfo {
        is_flush_draw,
        is_nut_flush_draw,
        is_straight_draw,
        is_gutshot,
    }
}

#[test]
fn test_lut_evaluator_draws_texture_and_bucket_parity() {
    let mut rng = SmallRng::seed_from_u64(0xE7A1_2026);
    let mut deck = ALL_52_CARDS;

    for deal_idx in 0..5_000 {
        deck.shuffle(&mut rng);
        let hole = [deck[0], deck[1]];
        for board_len in [3usize, 4, 5] {
            let board = &deck[2..2 + board_len];
            let fast_score = evaluate_7cards(&hole, board);
            assert_eq!(
                fast_score,
                reference_evaluate_7cards(hole, board),
                "deal {deal_idx}"
            );
            assert_eq!(
                detect_draws_detailed(&hole, board),
                reference_detect_draws_detailed(hole, board),
                "draws deal {deal_idx}"
            );

            let (mut suit_counts, mut rank_counts) = ([0u8; 4], [0u8; 13]);
            for &c in board {
                suit_counts[c.suit as usize] += 1;
                rank_counts[c.rank as usize] += 1;
            }
            let max_suit = *suit_counts.iter().max().unwrap();
            let expected_flush_tex = match max_suit {
                3.. => BoardFlushTexture::Monotone,
                2 => BoardFlushTexture::TwoTone,
                _ => BoardFlushTexture::Rainbow,
            };
            let expected_paired = rank_counts.iter().any(|&cnt| cnt >= 2);
            let expected_connected = (0..12).any(|r| rank_counts[r] > 0 && rank_counts[r + 1] > 0)
                || (rank_counts[12] > 0 && (rank_counts[0] > 0 || rank_counts[11] > 0));

            let tex = detect_board_texture(board);
            assert_eq!(tex.flush_texture, expected_flush_tex);
            assert_eq!(
                (tex.is_paired, tex.is_connected),
                (expected_paired, expected_connected)
            );
            assert_eq!(is_three_flush_board(board), max_suit >= 3);
            assert_eq!(is_four_flush_board(board), board_len >= 4 && max_suit >= 4);

            let win_cnt =
                |s: usize, len: usize| (s..s + len).filter(|&r| rank_counts[r] > 0).count();
            let wheel_cnt = |sl: &[usize]| sl.iter().filter(|&&r| rank_counts[r] > 0).count();
            let four_st = (0..=8).any(|s| win_cnt(s, 5) >= 4) || wheel_cnt(&[12, 0, 1, 2, 3]) >= 4;
            assert_eq!(is_four_straight_board(board), board_len >= 4 && four_st);

            let coord_paired = board_len >= 4
                && expected_paired
                && ((0..=9).any(|s| win_cnt(s, 4) >= 3) || wheel_cnt(&[12, 0, 1, 2]) >= 3);
            assert_eq!(
                is_dangerous_board(board),
                (max_suit >= 3) || four_st || coord_paired
            );
            assert_eq!(
                postflop_equity_bucket_with_score(&hole, board, fast_score),
                postflop_equity_bucket(&hole, board)
            );
        }
    }
}
