use lil_poker_mccfr::cfr::abstraction::{
    detect_board_texture, detect_draws, detect_draws_detailed, postflop_equity_bucket,
    BoardFlushTexture,
};
use lil_poker_mccfr::game::holdem::{evaluate_7cards, Card, Rank, Suit};

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
fn test_draw_detection_and_equity_buckets() {
    let card = |rank, suit| Card { rank, suit };
    /* Flush draw: A♠ 4♠ on K♠ 8♠ 2♦ */
    let hole_fd = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Four, Suit::Spades),
    ];
    let board_fd = [
        card(Rank::King, Suit::Spades),
        card(Rank::Eight, Suit::Spades),
        card(Rank::Two, Suit::Diamonds),
    ];
    let (is_fd, is_sd) = detect_draws(&hole_fd, &board_fd);
    assert!(is_fd, "Should detect flush draw");
    assert!(!is_sd, "Should not detect straight draw");
    let bucket_fd = postflop_equity_bucket(&hole_fd, &board_fd);
    assert!(
        bucket_fd >= 15,
        "Flush draw with Ace should be high tier draw (bucket >= 15), got {}",
        bucket_fd
    );

    /* One Pair AA vs One Pair 22 should NOT have the same bucket! */
    let hole_aa = [card(Rank::Ace, Suit::Hearts), card(Rank::Two, Suit::Clubs)];
    let board_a = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Nine, Suit::Clubs),
    ];
    let hole_22 = [
        card(Rank::Two, Suit::Hearts),
        card(Rank::Three, Suit::Clubs),
    ];
    let board_2 = [
        card(Rank::Two, Suit::Spades),
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Nine, Suit::Clubs),
    ];
    let bucket_aa = postflop_equity_bucket(&hole_aa, &board_a);
    let bucket_22 = postflop_equity_bucket(&hole_22, &board_2);
    assert!(
        bucket_aa > bucket_22,
        "Pair of Aces ({}) must be in higher bucket than Pair of Twos ({})",
        bucket_aa,
        bucket_22
    );
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
fn test_postflop_pair_hierarchy_and_board_pairs() {
    let card = |rank, suit| Card { rank, suit };

    /* Overpair: Pocket Queens on a Ten-high board */
    let hole_qq = [
        card(Rank::Queen, Suit::Spades),
        card(Rank::Queen, Suit::Hearts),
    ];
    let board_t = [
        card(Rank::Ten, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];
    let bucket_qq = postflop_equity_bucket(&hole_qq, &board_t);
    assert_eq!(bucket_qq, 43, "Overpair QQ on T-7-2 should be bucket 43");

    /* Top Pair Top Kicker: A-K on A-7-2 board */
    let hole_ak = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let board_a = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];
    let bucket_ak = postflop_equity_bucket(&hole_ak, &board_a);
    assert_eq!(bucket_ak, 39, "TPTK AK on A-7-2 should be bucket 39");

    /* Top Pair Weak Kicker: A-3 on A-7-2 board */
    let hole_a3 = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Three, Suit::Hearts),
    ];
    let bucket_a3 = postflop_equity_bucket(&hole_a3, &board_a);
    assert_eq!(bucket_a3, 36, "Top pair weak kicker should be bucket 36");
    assert!(
        bucket_ak > bucket_a3,
        "TPTK must be strictly higher than weak kicker top pair"
    );

    /* Board Pair: Board is A-A-K, player has 7-2 offsuit (0 aces, 0 kings).
     * Player should NOT be treated as having Top Pair Aces! */
    let hole_junk = [
        card(Rank::Seven, Suit::Spades),
        card(Rank::Two, Suit::Hearts),
    ];
    let board_aa_k = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Ace, Suit::Clubs),
        card(Rank::King, Suit::Hearts),
    ];
    let bucket_board_pair = postflop_equity_bucket(&hole_junk, &board_aa_k);
    assert!(
        bucket_board_pair < 36,
        "7-2 on A-A-K board must NOT be evaluated as a high pair (expected < 36, got {})",
        bucket_board_pair
    );
}

#[test]
fn test_board_texture_and_redraw_potential() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. Monotone Board */
    let board_mono = [
        card(Rank::King, Suit::Hearts),
        card(Rank::Eight, Suit::Hearts),
        card(Rank::Four, Suit::Hearts),
    ];
    let tex_mono = detect_board_texture(&board_mono);
    assert_eq!(tex_mono.flush_texture, BoardFlushTexture::Monotone);
    assert!(!tex_mono.is_paired);

    /* 2. Paired Connected Board */
    let board_pair = [
        card(Rank::Nine, Suit::Spades),
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Eight, Suit::Diamonds),
    ];
    let tex_pair = detect_board_texture(&board_pair);
    assert!(tex_pair.is_paired);
    assert!(tex_pair.is_connected);

    /* 3. Redraw potential: Top Pair + Flush Draw vs Dry Top Pair */
    let hole_tptk_fd = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::King, Suit::Hearts),
    ];
    let board_two_tone = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Two, Suit::Hearts),
    ];
    let bucket_with_fd = postflop_equity_bucket(&hole_tptk_fd, &board_two_tone);

    let hole_tptk_dry = [card(Rank::Ace, Suit::Spades), card(Rank::King, Suit::Clubs)];
    let bucket_dry = postflop_equity_bucket(&hole_tptk_dry, &board_two_tone);

    assert!(
        bucket_with_fd > bucket_dry,
        "TPTK with Nut Flush Draw ({}) must have higher bucket than dry TPTK ({})",
        bucket_with_fd,
        bucket_dry
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
        "AA-KQ9 (score1: {:x}) must defeat AA-KQ8 (score2: {:x}) by 3rd kicker",
        score1,
        score2
    );
}

#[test]
fn test_kicker_aware_flush_and_straight_abstraction() {
    let card = |rank, suit| Card { rank, suit };
    /* Monotone board with Ace: A♠ J♠ 10♠ 3♠ 2♠ */
    let board_mono = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Jack, Suit::Spades),
        card(Rank::Ten, Suit::Spades),
        card(Rank::Three, Suit::Spades),
        card(Rank::Two, Suit::Spades),
    ];

    let hole_king = [
        card(Rank::King, Suit::Spades),
        card(Rank::Four, Suit::Diamonds),
    ];
    let hole_queen = [
        card(Rank::Queen, Suit::Spades),
        card(Rank::Four, Suit::Diamonds),
    ];
    let hole_low = [
        card(Rank::Six, Suit::Spades),
        card(Rank::Four, Suit::Spades),
    ];
    let hole_board_only = [
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Eight, Suit::Diamonds),
    ];

    let b_king = postflop_equity_bucket(&hole_king, &board_mono);
    let b_queen = postflop_equity_bucket(&hole_queen, &board_mono);
    let b_low = postflop_equity_bucket(&hole_low, &board_mono);
    let b_board_only = postflop_equity_bucket(&hole_board_only, &board_mono);

    assert_eq!(
        b_king, 71,
        "King of spades on Ace-board must be Nut Flush (bucket 71)"
    );
    assert_eq!(
        b_queen, 70,
        "Queen of spades on Ace-board must be 2nd Nut Flush (bucket 70)"
    );
    assert_eq!(
        b_low, 67,
        "6s 4s on Ace-board must be Low Flush (bucket 67)"
    );
    assert_eq!(
        b_board_only, 67,
        "No spades in hand must be Board Flush (bucket 67)"
    );

    /* Straight board: 5♠ 6♥ 7♦ 8♣ 9♠ */
    let board_straight = [
        card(Rank::Five, Suit::Spades),
        card(Rank::Six, Suit::Hearts),
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
        card(Rank::Nine, Suit::Spades),
    ];
    let hole_nut_straight = [card(Rank::Ten, Suit::Spades), card(Rank::Two, Suit::Clubs)];
    let hole_board_straight = [
        card(Rank::Two, Suit::Hearts),
        card(Rank::Three, Suit::Clubs),
    ];

    let b_nut_st = postflop_equity_bucket(&hole_nut_straight, &board_straight);
    let b_board_st = postflop_equity_bucket(&hole_board_straight, &board_straight);

    assert!(
        b_nut_st > b_board_st,
        "Holding Ten must produce higher straight bucket ({}) than playing board ({})",
        b_nut_st,
        b_board_st
    );
    assert_eq!(
        b_board_st, 62,
        "Playing the board straight must map to bucket 62"
    );
}

#[test]
fn test_dangerous_board_detection() {
    use lil_poker_mccfr::cfr::abstraction::is_dangerous_board;

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

#[test]
fn test_pocket_pair_above_and_below_board_pair_buckets() {
    let card = |rank, suit| Card { rank, suit };

    /* Board has a pair of 8s: 8-8-2 */
    let board_paired = [
        card(Rank::Eight, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];

    /* Overpair to board pair: AA on 8-8-2 */
    let hole_aa = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts)];
    let bucket_aa = postflop_equity_bucket(&hole_aa, &board_paired);
    assert_eq!(
        bucket_aa, 54,
        "AA on 8-8-2 must be bucket 54 (Pocket pair above board pair), got {}",
        bucket_aa
    );

    /* Underpair to board pair: 55 on 8-8-2 */
    let hole_55 = [
        card(Rank::Five, Suit::Spades),
        card(Rank::Five, Suit::Hearts),
    ];
    let bucket_55 = postflop_equity_bucket(&hole_55, &board_paired);
    assert_eq!(
        bucket_55, 48,
        "55 on 8-8-2 must be bucket 48 (Pocket pair below board pair), got {}",
        bucket_55
    );

    /* Overpair KK on 8-8-2 */
    let hole_kk = [
        card(Rank::King, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let bucket_kk = postflop_equity_bucket(&hole_kk, &board_paired);
    assert_eq!(
        bucket_kk, 54,
        "KK on 8-8-2 must be bucket 54 (Pocket pair above board pair), got {}",
        bucket_kk
    );
}
