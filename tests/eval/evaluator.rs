use lil_poker_mccfr::cfr::abstraction::{
    BoardFlushTexture, DrawInfo, detect_board_texture, detect_draws, detect_draws_detailed,
    is_dangerous_board, is_four_flush_board, is_four_straight_board, is_three_flush_board,
    postflop_equity_bucket, postflop_equity_bucket_with_score,
};
use lil_poker_mccfr::game::holdem::{ALL_52_CARDS, Card, Rank, Suit, evaluate_7cards};
use rand::SeedableRng;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;

#[test]
fn test_evaluate_7cards_categories() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. Royal / Straight Flush: A♥ K♥ Q♥ J♥ T♥ 9♣ 2♦ */
    let hole_sf = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::King, Suit::Hearts),
    ];
    let board_sf = [
        card(Rank::Queen, Suit::Hearts),
        card(Rank::Jack, Suit::Hearts),
        card(Rank::Ten, Suit::Hearts),
        card(Rank::Nine, Suit::Clubs),
        card(Rank::Two, Suit::Diamonds),
    ];
    let score_sf = evaluate_7cards(&hole_sf, &board_sf);
    assert_eq!(score_sf >> 32, 8, "Expected Straight Flush category 8");

    /* 2. Four of a kind (Quads): 9♠ 9♥ 9♦ 9♣ K♣ 2♥ 3♦ */
    let hole_quads = [
        card(Rank::Nine, Suit::Spades),
        card(Rank::Nine, Suit::Hearts),
    ];
    let board_quads = [
        card(Rank::Nine, Suit::Diamonds),
        card(Rank::Nine, Suit::Clubs),
        card(Rank::King, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
        card(Rank::Three, Suit::Diamonds),
    ];
    let score_quads = evaluate_7cards(&hole_quads, &board_quads);
    assert_eq!(score_quads >> 32, 7, "Expected Quads category 7");

    /* 3. Full House: K♠ K♥ K♦ 4♣ 4♥ 2♦ 8♣ */
    let hole_fh = [
        card(Rank::King, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let board_fh = [
        card(Rank::King, Suit::Diamonds),
        card(Rank::Four, Suit::Clubs),
        card(Rank::Four, Suit::Hearts),
        card(Rank::Two, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
    ];
    let score_fh = evaluate_7cards(&hole_fh, &board_fh);
    assert_eq!(score_fh >> 32, 6, "Expected Full House category 6");

    /* 4. Flush: A♠ T♠ 8♠ 6♠ 2♠ K♥ Q♦ */
    let hole_fl = [card(Rank::Ace, Suit::Spades), card(Rank::Ten, Suit::Spades)];
    let board_fl = [
        card(Rank::Eight, Suit::Spades),
        card(Rank::Six, Suit::Spades),
        card(Rank::Two, Suit::Spades),
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
    ];
    let score_fl = evaluate_7cards(&hole_fl, &board_fl);
    assert_eq!(score_fl >> 32, 5, "Expected Flush category 5");

    /* 5. Straight (Ace-low A-2-3-4-5): A♠ 2♥ 3♦ 4♣ 5♠ K♥ 8♦ */
    let hole_st = [card(Rank::Ace, Suit::Spades), card(Rank::Two, Suit::Hearts)];
    let board_st = [
        card(Rank::Three, Suit::Diamonds),
        card(Rank::Four, Suit::Clubs),
        card(Rank::Five, Suit::Spades),
        card(Rank::King, Suit::Hearts),
        card(Rank::Eight, Suit::Diamonds),
    ];
    let score_st = evaluate_7cards(&hole_st, &board_st);
    assert_eq!(score_st >> 32, 4, "Expected Straight category 4");
    assert_eq!(score_st & 0xFF, 3, "Expected 5-high straight");

    /* 6. Three of a kind: Q♠ Q♥ Q♦ 9♣ 7♥ 4♦ 2♣ */
    let hole_trips = [
        card(Rank::Queen, Suit::Spades),
        card(Rank::Queen, Suit::Hearts),
    ];
    let board_trips = [
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Nine, Suit::Clubs),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Four, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    ];
    let score_trips = evaluate_7cards(&hole_trips, &board_trips);
    assert_eq!(score_trips >> 32, 3, "Expected Trips category 3");

    /* 7. Two Pair: J♠ J♥ 8♦ 8♣ A♣ 4♦ 2♥ */
    let hole_tp = [
        card(Rank::Jack, Suit::Spades),
        card(Rank::Jack, Suit::Hearts),
    ];
    let board_tp = [
        card(Rank::Eight, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
        card(Rank::Ace, Suit::Clubs),
        card(Rank::Four, Suit::Diamonds),
        card(Rank::Two, Suit::Hearts),
    ];
    let score_tp = evaluate_7cards(&hole_tp, &board_tp);
    assert_eq!(score_tp >> 32, 2, "Expected Two Pair category 2");

    /* 8. One Pair: T♠ T♥ A♦ 8♣ 6♣ 4♦ 2♥ */
    let hole_op = [card(Rank::Ten, Suit::Spades), card(Rank::Ten, Suit::Hearts)];
    let board_op = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
        card(Rank::Six, Suit::Clubs),
        card(Rank::Four, Suit::Diamonds),
        card(Rank::Two, Suit::Hearts),
    ];
    let score_op = evaluate_7cards(&hole_op, &board_op);
    assert_eq!(score_op >> 32, 1, "Expected One Pair category 1");

    /* 9. High Card: A♠ K♥ J♦ 8♣ 6♣ 4♦ 2♥ */
    let hole_hc = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let board_hc = [
        card(Rank::Jack, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
        card(Rank::Six, Suit::Clubs),
        card(Rank::Four, Suit::Diamonds),
        card(Rank::Two, Suit::Hearts),
    ];
    let score_hc = evaluate_7cards(&hole_hc, &board_hc);
    assert_eq!(score_hc >> 32, 0, "Expected High Card category 0");
}

#[test]
fn test_player_draw_contribution() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. Board has 4 hearts, but player has NO hearts:
     * Player does NOT have a flush draw! */
    let hole_no_h = [
        card(Rank::King, Suit::Spades),
        card(Rank::Nine, Suit::Diamonds),
    ];
    let board_4h = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Five, Suit::Hearts),
        card(Rank::Two, Suit::Hearts),
    ];
    let (is_fd, _) = detect_draws(&hole_no_h, &board_4h);
    assert!(
        !is_fd,
        "Player with no hearts must NOT be credited with a flush draw on a 4-heart board"
    );

    /* 2. Board has 3 hearts, player has 1 heart:
     * Player DOES have a flush draw! */
    let hole_1h = [
        card(Rank::King, Suit::Hearts),
        card(Rank::Nine, Suit::Diamonds),
    ];
    let board_3h = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Five, Suit::Hearts),
    ];
    let (is_fd2, _) = detect_draws(&hole_1h, &board_3h);
    assert!(
        is_fd2,
        "Player with 1 heart on a 3-heart board must have a flush draw"
    );
}

#[test]
fn test_bitwise_evaluate_7cards_tiebreakers() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. Straight: Broadway A-K-Q-J-T vs King-high K-Q-J-T-9 vs Wheel 5-4-3-2-A */
    let hole_broadway = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let board_broadway = [
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Jack, Suit::Clubs),
        card(Rank::Ten, Suit::Hearts),
        card(Rank::Four, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];
    let score_broadway = evaluate_7cards(&hole_broadway, &board_broadway);
    assert_eq!(score_broadway >> 32, 4);
    assert_eq!(
        score_broadway & 0xFF,
        12,
        "Broadway high rank must be 12 (Ace)"
    );

    let hole_wheel = [card(Rank::Ace, Suit::Spades), card(Rank::Two, Suit::Hearts)];
    let board_wheel = [
        card(Rank::Three, Suit::Diamonds),
        card(Rank::Four, Suit::Clubs),
        card(Rank::Five, Suit::Hearts),
        card(Rank::Nine, Suit::Spades),
        card(Rank::Eight, Suit::Clubs),
    ];
    let score_wheel = evaluate_7cards(&hole_wheel, &board_wheel);
    assert_eq!(score_wheel >> 32, 4);
    assert_eq!(score_wheel & 0xFF, 3, "Wheel high rank must be 3 (Five)");
    assert!(score_broadway > score_wheel, "Broadway must beat Wheel");

    /* 2. Flush tiebreakers: A♠ K♠ Q♠ J♠ 9♠ vs A♠ K♠ Q♠ J♠ 8♠ */
    let hole_fl1 = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Nine, Suit::Spades),
    ];
    let hole_fl2 = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Eight, Suit::Spades),
    ];
    let board_fl = [
        card(Rank::King, Suit::Spades),
        card(Rank::Queen, Suit::Spades),
        card(Rank::Jack, Suit::Spades),
        card(Rank::Three, Suit::Diamonds),
        card(Rank::Two, Suit::Hearts),
    ];
    let score_fl1 = evaluate_7cards(&hole_fl1, &board_fl);
    let score_fl2 = evaluate_7cards(&hole_fl2, &board_fl);
    assert_eq!(score_fl1 >> 32, 5);
    assert_eq!(score_fl2 >> 32, 5);
    assert!(score_fl1 > score_fl2, "Higher 5th flush card must win");

    /* 3. Full House: KKK22 vs QQQAA (Trips rank decides) */
    let hole_fh_k = [
        card(Rank::King, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let board_fh_k = [
        card(Rank::King, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
        card(Rank::Two, Suit::Diamonds),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Eight, Suit::Clubs),
    ];
    let hole_fh_q = [
        card(Rank::Queen, Suit::Spades),
        card(Rank::Queen, Suit::Hearts),
    ];
    let board_fh_q = [
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Ace, Suit::Clubs),
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Eight, Suit::Clubs),
    ];
    let score_fh_k = evaluate_7cards(&hole_fh_k, &board_fh_k);
    let score_fh_q = evaluate_7cards(&hole_fh_q, &board_fh_q);
    assert_eq!(score_fh_k >> 32, 6);
    assert_eq!(score_fh_q >> 32, 6);
    assert!(score_fh_k > score_fh_q, "KKK22 must beat QQQAA");

    /* 4. Three pairs on 7 cards (AA KK QQ): best 2 pairs chosen (AA-KK) with Q kicker */
    let hole_3p1 = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Spades),
    ];
    let board_3p = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    ];
    let score_3p1 = evaluate_7cards(&hole_3p1, &board_3p);
    assert_eq!(score_3p1 >> 32, 2, "Must be Two Pair category 2");
    assert_eq!((score_3p1 >> 16) & 0xFF, 12, "Top pair Ace (12)");
    assert_eq!((score_3p1 >> 8) & 0xFF, 11, "Second pair King (11)");
    assert_eq!(score_3p1 & 0xFF, 10, "Kicker Queen (10)");
}

#[test]
fn test_one_pair_third_kicker_tiebreaker() {
    let card = |rank, suit| Card { rank, suit };
    let board = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
        card(Rank::Three, Suit::Clubs),
    ];
    let hole1 = [card(Rank::Ace, Suit::Clubs), card(Rank::Nine, Suit::Spades)];
    let hole2 = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Eight, Suit::Spades),
    ];

    let score1 = evaluate_7cards(&hole1, &board);
    let score2 = evaluate_7cards(&hole2, &board);

    assert_eq!(score1 >> 32, 1, "Must be One Pair category 1");
    assert_eq!(score2 >> 32, 1, "Must be One Pair category 1");

    /* Both hands have AA KQ, but hole1 has 9 kicker while hole2 has 8 kicker */
    assert!(
        score1 > score2,
        "AA-KQ9 (score1: {score1:x}) must defeat AA-KQ8 (score2: {score2:x}) by 3rd kicker"
    );
}

#[test]
fn test_dangerous_board_detection() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. 4-to-straight board: 4d 6d 7c 3c (any 5 makes straight) */
    let board_wet_straight = [
        card(Rank::Four, Suit::Diamonds),
        card(Rank::Six, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Three, Suit::Clubs),
    ];
    assert!(
        is_dangerous_board(&board_wet_straight),
        "4-to-straight board must be detected as dangerous"
    );

    /* 2. 4-to-flush board */
    let board_wet_flush = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Six, Suit::Hearts),
        card(Rank::Two, Suit::Hearts),
        card(Rank::King, Suit::Clubs),
    ];
    assert!(
        is_dangerous_board(&board_wet_flush),
        "4-to-flush board must be detected as dangerous"
    );

    /* 3. Dry, disconnected board: K♠ 8♦ 2♣ */
    let board_dry = [
        card(Rank::King, Suit::Spades),
        card(Rank::Eight, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    ];
    assert!(
        !is_dangerous_board(&board_dry),
        "Dry rainbow disconnected board must NOT be dangerous"
    );
}

#[test]
fn test_wheel_and_broadway_gutshot_vs_open_ended_draws() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. Wheel Draw: A-2 on 3-4-K board.
     * Only 5 completes the straight (4 outs).
     * Must be is_gutshot = true, is_straight_draw = false. */
    let hole_wheel = [card(Rank::Ace, Suit::Spades), card(Rank::Two, Suit::Hearts)];
    let board_wheel = [
        card(Rank::Three, Suit::Clubs),
        card(Rank::Four, Suit::Diamonds),
        card(Rank::King, Suit::Spades),
    ];
    let draw_wheel = detect_draws_detailed(&hole_wheel, &board_wheel);
    assert!(
        draw_wheel.is_gutshot,
        "A-2 on 3-4-K is a 4-out gutshot to the wheel straight"
    );
    assert!(
        !draw_wheel.is_straight_draw,
        "A-2 on 3-4-K must NOT be classified as an 8-out open-ended straight draw"
    );

    /* 2. Broadway Gutshot: A-J on K-Q-2 board.
     * Only 10 completes the straight (4 outs).
     * Must be is_gutshot = true, is_straight_draw = false. */
    let hole_broadway = [
        card(Rank::Ace, Suit::Clubs),
        card(Rank::Jack, Suit::Diamonds),
    ];
    let board_broadway = [
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];
    let draw_broadway = detect_draws_detailed(&hole_broadway, &board_broadway);
    assert!(draw_broadway.is_gutshot, "A-J on K-Q-2 is a 4-out gutshot");
    assert!(
        !draw_broadway.is_straight_draw,
        "A-J on K-Q-2 must NOT be classified as an 8-out open-ended straight draw"
    );

    /* 3. True Open-Ended Straight Draw: 6-7 on 8-9-2 board.
     * Both 5 and 10 complete the straight (8 outs).
     * Must be is_straight_draw = true. */
    let hole_oesd = [
        card(Rank::Six, Suit::Clubs),
        card(Rank::Seven, Suit::Diamonds),
    ];
    let board_oesd = [
        card(Rank::Eight, Suit::Hearts),
        card(Rank::Nine, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];
    let draw_oesd = detect_draws_detailed(&hole_oesd, &board_oesd);
    assert!(
        draw_oesd.is_straight_draw,
        "6-7 on 8-9-2 is an 8-out open-ended straight draw"
    );

    /* 4. Low Open-Ended Straight Draw: 2-3 on 4-5-K board.
     * Both Ace and 6 complete the straight (8 outs).
     * Must be is_straight_draw = true. */
    let hole_low_oesd = [
        card(Rank::Two, Suit::Hearts),
        card(Rank::Three, Suit::Diamonds),
    ];
    let board_low_oesd = [
        card(Rank::Four, Suit::Clubs),
        card(Rank::Five, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let draw_low_oesd = detect_draws_detailed(&hole_low_oesd, &board_low_oesd);
    assert!(
        draw_low_oesd.is_straight_draw,
        "2-3 on 4-5-K has 8 outs (Ace or 6) and is open-ended"
    );
}

fn reference_evaluate_7cards(hole: [Card; 2], board: &[Card]) -> u64 {
    let board_n = board.len().min(5);
    let total_cards = 2 + board_n;
    let mut rank_counts = [0u8; 13];
    let mut suit_counts = [0u8; 4];
    let mut rank_mask = 0u16;
    let mut suit_masks = [0u16; 4];

    for c in hole.iter().chain(board[..board_n].iter()) {
        let r = c.rank as usize;
        let s = c.suit as usize;
        rank_counts[r] += 1;
        suit_counts[s] += 1;
        rank_mask |= 1 << r;
        suit_masks[s] |= 1 << r;
    }

    let flush_suit = suit_counts.iter().position(|&cnt| cnt >= 5);
    if let Some(s) = flush_suit {
        let smask = suit_masks[s];
        let st_mask = smask & (smask >> 1) & (smask >> 2) & (smask >> 3) & (smask >> 4);
        if st_mask != 0 {
            let top_bit = u64::from(15 - st_mask.leading_zeros());
            return (8 << 32) | (top_bit + 4);
        }
        if (smask & 0x100F) == 0x100F {
            return (8 << 32) | 3;
        }
    }

    let mut quads = None;
    let mut trips = [0u64; 2];
    let mut trips_len = 0usize;
    let mut pairs = [0u64; 3];
    let mut pairs_len = 0usize;
    for r in (0..13).rev() {
        match rank_counts[r] {
            4 => quads = Some(r as u64),
            3 => {
                trips[trips_len] = r as u64;
                trips_len += 1;
            }
            2 => {
                pairs[pairs_len] = r as u64;
                pairs_len += 1;
            }
            _ => {}
        }
    }

    if let Some(q) = quads {
        let rem_mask = rank_mask & !(1 << q);
        let kicker = if rem_mask != 0 {
            u64::from(15 - rem_mask.leading_zeros())
        } else {
            0
        };
        return (7 << 32) | (q << 16) | kicker;
    }

    if trips_len >= 2 {
        return (6 << 32) | (trips[0] << 16) | trips[1];
    }
    if trips_len == 1 && pairs_len >= 1 {
        return (6 << 32) | (trips[0] << 16) | pairs[0];
    }

    if let Some(s) = flush_suit {
        let mut score = 5u64 << 32;
        let mut m = suit_masks[s];
        for i in 0..5 {
            let r = u64::from(15 - m.leading_zeros());
            score |= r << (16 - i * 4);
            m &= !(1 << r);
        }
        return score;
    }

    let st_mask =
        rank_mask & (rank_mask >> 1) & (rank_mask >> 2) & (rank_mask >> 3) & (rank_mask >> 4);
    if st_mask != 0 {
        let top_bit = u64::from(15 - st_mask.leading_zeros());
        return (4 << 32) | (top_bit + 4);
    }
    if (rank_mask & 0x100F) == 0x100F {
        return (4 << 32) | 3;
    }

    if trips_len > 0 {
        let t = trips[0];
        let mut rem_mask = rank_mask & !(1 << t);
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

    if pairs_len >= 2 {
        let p1 = pairs[0];
        let p2 = pairs[1];
        let rem_mask = rank_mask & !((1 << p1) | (1 << p2));
        let kicker = if rem_mask != 0 {
            u64::from(15 - rem_mask.leading_zeros())
        } else {
            0
        };
        return (2 << 32) | (p1 << 16) | (p2 << 8) | kicker;
    }

    if pairs_len >= 1 {
        let p = pairs[0];
        let mut rem_mask = rank_mask & !(1 << p);
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
        return (1 << 32) | (p << 16) | (k0 << 12) | (k1 << 8) | (k2 << 4);
    }

    let mut score = 0u64;
    let mut rem_mask = rank_mask;
    for i in 0..total_cards.min(5) {
        if rem_mask == 0 {
            break;
        }
        let r = u64::from(15 - rem_mask.leading_zeros());
        score |= r << (16 - i * 4);
        rem_mask &= !(1 << r);
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

    let mut board_suit_counts = [0u8; 4];
    for &b in board {
        board_suit_counts[b.suit as usize] += 1;
    }

    let mut is_flush_draw = false;
    let mut is_nut_flush_draw = false;
    for (s, &b_count) in board_suit_counts.iter().enumerate() {
        let hole_has = u8::from(hole[0].suit as usize == s) + u8::from(hole[1].suit as usize == s);
        if hole_has >= 1 && (b_count + hole_has == 4) {
            let max_hole_suit_rank = hole
                .iter()
                .filter(|c| c.suit as usize == s)
                .map(|c| c.rank as usize)
                .max()
                .unwrap_or(0);
            let ace_on_board = board
                .iter()
                .any(|c| c.suit as usize == s && c.rank == Rank::Ace);
            let is_nut_card = max_hole_suit_rank == (Rank::Ace as usize)
                || (ace_on_board && max_hole_suit_rank == (Rank::King as usize));
            if hole_has == 2
                || max_hole_suit_rank >= (Rank::King as usize)
                || (board.len() == 3 && max_hole_suit_rank >= (Rank::Jack as usize))
            {
                is_flush_draw = true;
                if is_nut_card {
                    is_nut_flush_draw = true;
                }
            }
            break;
        }
    }

    let mut board_ranks = [false; 13];
    let mut ranks = [false; 13];
    for &b in board {
        board_ranks[b.rank as usize] = true;
        ranks[b.rank as usize] = true;
    }
    let h0 = hole[0].rank as usize;
    let h1 = hole[1].rank as usize;
    ranks[h0] = true;
    ranks[h1] = true;

    let mut is_straight_draw = false;
    for r in 0..9 {
        if ranks[r]
            && ranks[r + 1]
            && ranks[r + 2]
            && ranks[r + 3]
            && ((h0 >= r && h0 <= r + 3 && !board_ranks[h0])
                || (h1 >= r && h1 <= r + 3 && !board_ranks[h1]))
        {
            is_straight_draw = true;
            break;
        }
    }

    let mut is_gutshot = false;
    if !is_straight_draw {
        for r in 0..9 {
            let count = u8::from(ranks[r])
                + u8::from(ranks[r + 1])
                + u8::from(ranks[r + 2])
                + u8::from(ranks[r + 3])
                + u8::from(ranks[r + 4]);
            if count == 4
                && ((h0 >= r && h0 <= r + 4 && !board_ranks[h0])
                    || (h1 >= r && h1 <= r + 4 && !board_ranks[h1]))
            {
                is_gutshot = true;
                break;
            }
        }
        if !is_gutshot && ranks[12] {
            let count = 1
                + u8::from(ranks[0])
                + u8::from(ranks[1])
                + u8::from(ranks[2])
                + u8::from(ranks[3]);
            if count == 4
                && (((h0 == 12 || h0 <= 3) && !board_ranks[h0])
                    || ((h1 == 12 || h1 <= 3) && !board_ranks[h1]))
            {
                is_gutshot = true;
            }
        }
    }

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
            let ref_score = reference_evaluate_7cards(hole, board);
            assert_eq!(
                fast_score, ref_score,
                "Evaluator score mismatch at deal {deal_idx} (len {board_len})"
            );

            let fast_draws = detect_draws_detailed(&hole, board);
            let ref_draws = reference_detect_draws_detailed(hole, board);
            assert_eq!(
                fast_draws, ref_draws,
                "DrawInfo mismatch at deal {deal_idx} (len {board_len})"
            );

            let mut suit_counts = [0u8; 4];
            let mut rank_counts = [0u8; 13];
            for &c in board {
                suit_counts[c.suit as usize] += 1;
                rank_counts[c.rank as usize] += 1;
            }
            let max_suit = *suit_counts.iter().max().unwrap();
            let expected_flush_tex = if max_suit >= 3 {
                BoardFlushTexture::Monotone
            } else if max_suit == 2 {
                BoardFlushTexture::TwoTone
            } else {
                BoardFlushTexture::Rainbow
            };
            let expected_paired = rank_counts.iter().any(|&cnt| cnt >= 2);
            let mut expected_connected = false;
            let mut consec = 0usize;
            for &cnt in &rank_counts {
                if cnt > 0 {
                    consec += 1;
                    if consec >= 2 {
                        expected_connected = true;
                        break;
                    }
                } else {
                    consec = 0;
                }
            }
            if !expected_connected
                && rank_counts[12] > 0
                && (rank_counts[0] > 0 || rank_counts[11] > 0)
            {
                expected_connected = true;
            }

            let tex = detect_board_texture(board);
            assert_eq!(tex.flush_texture, expected_flush_tex);
            assert_eq!(tex.is_paired, expected_paired);
            assert_eq!(tex.is_connected, expected_connected);
            assert_eq!(is_three_flush_board(board), max_suit >= 3);
            assert_eq!(is_four_flush_board(board), board_len >= 4 && max_suit >= 4);

            let four_st_any = (0..=8)
                .any(|s| (s..s + 5).filter(|&r| rank_counts[r] > 0).count() >= 4)
                || [12usize, 0, 1, 2, 3]
                    .iter()
                    .filter(|&&r| rank_counts[r] > 0)
                    .count()
                    >= 4;
            assert_eq!(is_four_straight_board(board), board_len >= 4 && four_st_any);

            let coord_paired = board_len >= 4
                && expected_paired
                && ((0..=9).any(|s| (s..s + 4).filter(|&r| rank_counts[r] > 0).count() >= 3)
                    || [12usize, 0, 1, 2]
                        .iter()
                        .filter(|&&r| rank_counts[r] > 0)
                        .count()
                        >= 3);
            assert_eq!(
                is_dangerous_board(board),
                (max_suit >= 3) || four_st_any || coord_paired,
                "is_dangerous_board mismatch at deal {deal_idx} (len {board_len})"
            );

            assert_eq!(
                postflop_equity_bucket_with_score(&hole, board, fast_score),
                postflop_equity_bucket(&hole, board),
                "postflop_equity_bucket_with_score mismatch at deal {deal_idx}"
            );
        }
    }
}
