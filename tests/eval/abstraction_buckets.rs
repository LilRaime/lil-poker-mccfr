use lil_poker_mccfr::cfr::abstraction::{
    BoardFlushTexture, calculate_range_equity, detect_board_texture, detect_draws,
    get_holdem_infoset_key, nash_call_shove_max_bb, nash_open_push_max_bb, postflop_equity_bucket,
    preflop_bucket,
};
use lil_poker_mccfr::game::holdem::{Card, Rank, Suit};

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
        "Flush draw with Ace should be high tier draw (bucket >= 15), got {bucket_fd}"
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
        "Pair of Aces ({bucket_aa}) must be in higher bucket than Pair of Twos ({bucket_22})"
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
    assert_eq!(
        bucket_qq, 54,
        "Dry Overpair QQ on T-7-2 should be bucket 54"
    );

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
    assert_eq!(bucket_ak, 47, "Dry TPTK AK on A-7-2 should be bucket 47");

    /* Top Pair Weak Kicker: A-3 on A-7-2 board */
    let hole_a3 = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Three, Suit::Hearts),
    ];
    let bucket_a3 = postflop_equity_bucket(&hole_a3, &board_a);
    assert_eq!(
        bucket_a3, 44,
        "Dry Top pair weak kicker should be bucket 44"
    );
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
        bucket_board_pair <= 10,
        "7-2 on A-A-K board must be evaluated as unpaired high card (<= 10, got {bucket_board_pair})"
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
        "TPTK with Nut Flush Draw ({bucket_with_fd}) must have higher bucket than dry TPTK ({bucket_dry})"
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
        b_king, 83,
        "King of spades on Ace-board must be Nut Flush (bucket 83)"
    );
    assert_eq!(
        b_queen, 82,
        "Queen of spades on Ace-board must be 2nd Nut Flush (bucket 82)"
    );
    assert_eq!(
        b_low, 79,
        "6s 4s on Ace-board must be Low Flush (bucket 79)"
    );
    assert_eq!(
        b_board_only, 79,
        "No spades in hand must be Board Flush (bucket 79)"
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
        "Holding Ten must produce higher straight bucket ({b_nut_st}) than playing board ({b_board_st})"
    );
    assert_eq!(
        b_board_st, 73,
        "Playing the board straight must map to bucket 73"
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
        bucket_aa, 52,
        "AA on 8-8-2 must be bucket 52 (Vulnerable High Overpair on paired board), got {bucket_aa}"
    );

    /* Underpair to board pair: 55 on 8-8-2 */
    let hole_55 = [
        card(Rank::Five, Suit::Spades),
        card(Rank::Five, Suit::Hearts),
    ];
    let bucket_55 = postflop_equity_bucket(&hole_55, &board_paired);
    assert_eq!(
        bucket_55, 37,
        "55 on 8-8-2 must be bucket 37 (Pocket pair below 1 board overcard rank), got {bucket_55}"
    );

    /* Overpair KK on 8-8-2 */
    let hole_kk = [
        card(Rank::King, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let bucket_kk = postflop_equity_bucket(&hole_kk, &board_paired);
    assert_eq!(
        bucket_kk, 52,
        "KK on 8-8-2 must be bucket 52 (Vulnerable High Overpair on paired board), got {bucket_kk}"
    );
}

#[test]
fn test_range_equity_and_nash_short_stack_thresholds() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. Range-aware equity: Top pair on dry river has higher equity vs loose VPIP than vs tight range facing heavy bet */
    let hole_tp = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Jack, Suit::Diamonds),
    ];
    let board_river = [
        card(Rank::Ace, Suit::Clubs),
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Four, Suit::Spades),
        card(Rank::Two, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
    ];
    let eq_vs_loose = calculate_range_equity(&hole_tp, &board_river, 0.70, false, 1);
    let eq_vs_tight_bet = calculate_range_equity(&hole_tp, &board_river, 0.20, true, 1);
    assert!(
        eq_vs_loose > eq_vs_tight_bet,
        "Equity against wide passive range ({eq_vs_loose:.3}) must exceed equity against tight aggressive range ({eq_vs_tight_bet:.3})"
    );

    /* Multi-way equity discount must reduce expected equity compared to heads-up */
    let eq_multiway = calculate_range_equity(&hole_tp, &board_river, 0.50, false, 3);
    assert!(
        eq_multiway < eq_vs_loose,
        "Multi-way equity ({eq_multiway:.3}) must be lower than heads-up equity ({eq_vs_loose:.3})"
    );

    /* 2. Nash short-stack push/fold thresholds */
    let (aa_idx, _) = preflop_bucket(card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts));
    let (a5s_idx, _) = preflop_bucket(
        card(Rank::Ace, Suit::Spades),
        card(Rank::Five, Suit::Spades),
    );
    let (trash_idx, _) = preflop_bucket(
        card(Rank::Seven, Suit::Spades),
        card(Rank::Two, Suit::Hearts),
    );

    assert!(nash_open_push_max_bb(aa_idx, 1) >= 15);
    assert!(nash_open_push_max_bb(a5s_idx, 1) >= 12);
    assert_eq!(nash_open_push_max_bb(trash_idx, 1), 0);

    assert!(nash_call_shove_max_bb(aa_idx, 1, false) >= 50);
    assert!(
        nash_call_shove_max_bb(a5s_idx, 1, true) > nash_call_shove_max_bb(a5s_idx, 1, false),
        "Maniac adjustment must widen short-stack shove calling threshold"
    );
    assert_eq!(nash_call_shove_max_bb(trash_idx, 1, true), 2);
}

#[test]
fn test_backdoor_draws_and_trips_vs_set_bucket_separation() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. Flop Nut Backdoor Flush Draw + Overcard (Ah Td on Qs 7h 2h):
     * Must land in weak draw buckets (11..=14) on the Flop, and drop to High Card (<= 10) on the Turn if bricked */
    let hole_bdfd = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Ten, Suit::Diamonds),
    ];
    let board_flop = [
        card(Rank::Queen, Suit::Spades),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Two, Suit::Hearts),
    ];
    let b_flop = postflop_equity_bucket(&hole_bdfd, &board_flop);
    assert!(
        (11..=14).contains(&b_flop),
        "Flop Nut Backdoor FD + Overcard must be in weak draw range 11..=14, got {b_flop}"
    );

    let board_turn_brick = [
        card(Rank::Queen, Suit::Spades),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Two, Suit::Hearts),
        card(Rank::Four, Suit::Clubs),
    ];
    let b_turn = postflop_equity_bucket(&hole_bdfd, &board_turn_brick);
    assert!(
        b_turn <= 10,
        "Bricked backdoor draw on Turn must revert to High Card (<= 10), got {b_turn}"
    );

    /* 2. Strict separation between Visible Trips (65..=67) and Disguised Sets (68..=72) */
    let board_paired = [
        card(Rank::Nine, Suit::Spades),
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Four, Suit::Diamonds),
    ];
    let hole_trips_top = [
        card(Rank::Nine, Suit::Clubs),
        card(Rank::Ace, Suit::Diamonds),
    ];
    let board_unpaired = [
        card(Rank::King, Suit::Spades),
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Four, Suit::Diamonds),
    ];
    let hole_low_set = [
        card(Rank::Four, Suit::Clubs),
        card(Rank::Four, Suit::Spades),
    ];

    let b_trips = postflop_equity_bucket(&hole_trips_top, &board_paired);
    let b_set = postflop_equity_bucket(&hole_low_set, &board_unpaired);
    assert!(
        (65..=67).contains(&b_trips),
        "Visible trips without flush redraw must be in 65..=67, got {b_trips}"
    );
    assert!(
        (68..=72).contains(&b_set),
        "Disguised pocket-pair set must be in 68..=72, got {b_set}"
    );
    assert!(
        b_set > b_trips,
        "Disguised low set ({b_set}) must rank strictly higher than visible top-kicker trips ({b_trips})"
    );
}

#[test]
fn test_postflop_board_quads_and_double_paired_draw() {
    let board_quads = [
        Card::new(Rank::Seven, Suit::Spades),
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Two, Suit::Spades),
    ];
    let hole_weak = [
        Card::new(Rank::Five, Suit::Hearts),
        Card::new(Rank::Four, Suit::Diamonds),
    ];
    let hole_ace = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::King, Suit::Diamonds),
    ];
    let weak_quad_bucket = postflop_equity_bucket(&hole_weak, &board_quads);
    let ace_quad_bucket = postflop_equity_bucket(&hole_ace, &board_quads);
    assert!(
        weak_quad_bucket < 15,
        "Board quads with weak 5-4 kicker must be trash bucket < 15, got {weak_quad_bucket}"
    );
    assert!(
        ace_quad_bucket >= 50,
        "Board quads with Ace-high kicker must remain strong >= 50, got {ace_quad_bucket}"
    );

    let board_double_paired = [
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Nine, Suit::Diamonds),
        Card::new(Rank::Four, Suit::Spades),
        Card::new(Rank::Four, Suit::Clubs),
    ];
    let hole_nut_fd = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Spades),
    ];
    let fd_bucket = postflop_equity_bucket(&hole_nut_fd, &board_double_paired);
    assert_eq!(
        fd_bucket, 23,
        "Nut flush draw on double-paired board must preserve FD bucket 23"
    );
}

#[test]
fn test_full_house_pair_rank_and_counterfeited_pocket_pair_buckets() {
    /* 1. Top Full House on trips board (AA on K K K 7 2) must be bucket 85 (not 84 underfull) */
    let board_trips = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Two, Suit::Spades),
    ];
    let hole_aa = [
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::Ace, Suit::Diamonds),
    ];
    let hole_33 = [
        Card::new(Rank::Three, Suit::Clubs),
        Card::new(Rank::Three, Suit::Diamonds),
    ];
    assert_eq!(
        postflop_equity_bucket(&hole_aa, &board_trips),
        85,
        "AA on KKK72 must be top Full House (bucket 85)"
    );
    assert_eq!(
        postflop_equity_bucket(&hole_33, &board_trips),
        84,
        "33 on KKK72 is an underfull (3 < 7) -> bucket 84"
    );

    /* 2. Counterfeited pocket pair (22 on K K Q Q 7) must be classified as air/kicker (<= 10), not a live pair */
    let board_double_pair = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Queen, Suit::Diamonds),
        Card::new(Rank::Queen, Suit::Clubs),
        Card::new(Rank::Seven, Suit::Spades),
    ];
    let hole_22 = [
        Card::new(Rank::Two, Suit::Clubs),
        Card::new(Rank::Two, Suit::Hearts),
    ];
    let b_22 = postflop_equity_bucket(&hole_22, &board_double_pair);
    assert!(
        b_22 <= 10,
        "Counterfeited pocket pair 22 on KKQQ7 must be bucket <= 10, got {b_22}"
    );
}

#[test]
fn test_two_pair_bottom_pair_on_board_abstraction() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    /* Board has pair of Twos; player hits top pair Ace with King kicker */
    let board = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
        card(Rank::Ten, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];
    assert_eq!(
        postflop_equity_bucket(&hole, &board),
        43,
        "Top Pair Top Kicker on paired board must map to bucket 43, not Two Pair (63)"
    );
    let river_history = [vec![2u8, 1u8], vec![4u8, 1u8], vec![1u8, 1u8], vec![3u8]];
    let river_key = get_holdem_infoset_key(&hole, &board, 4, &river_history);
    /* Should be bucket 43 (Vulnerable Top Pair Top Kicker on paired board), NOT Genuine Two Pair (63) */
    assert_eq!(river_key, "R:B43/t");
}

#[test]
fn test_extended_blocker_and_potential_buckets() {
    use lil_poker_mccfr::cfr::abstraction::{
        BUCKET_DOUBLE_GUTSHOT, BUCKET_FD_TWO_OVERCARDS, BUCKET_FLOP_OVERPAIR_BDFD,
        BUCKET_FLOP_TOP_PAIR_BDFD, BUCKET_FLOP_TWO_OVERCARDS_AIR, BUCKET_FLOP_WEAK_PAIR_BDFD,
        BUCKET_LOW_BOARD_TOP_PAIR, BUCKET_NFD_TWO_OVERCARDS, BUCKET_PAIR_GUTSHOT,
        BUCKET_PAIRED_BOARD_NON_NUT_FLUSH, BUCKET_PAIRED_BOARD_NUT_FLUSH,
        BUCKET_RIVER_NUT_FLUSH_BLOCKER_AIR, BUCKET_RIVER_NUT_FLUSH_BLOCKER_TOP_PAIR,
        BUCKET_RIVER_NUT_FLUSH_BLOCKER_TWO_PAIR_PLUS, BUCKET_RIVER_NUT_FLUSH_BLOCKER_WEAK_PAIR,
        BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_AIR, BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_TOP_PAIR,
        BUCKET_RIVER_SECOND_NUT_FLUSH_BLOCKER_AIR, BUCKET_RIVER_STRAIGHT_NUT_FLUSH_BLOCKER,
        BUCKET_STRICT_TOP_TWO_PAIR, BUCKET_THREE_FLUSH_STRAIGHT, BUCKET_TOP_FULL_HOUSE,
        BUCKET_TOP_PAIR_NUT_FD, BUCKET_TRUE_TOP_SET, postflop_extended_bucket,
    };

    let c = |rank, suit| Card { rank, suit };
    let three_spade_river = [
        c(Rank::King, Suit::Spades),
        c(Rank::Nine, Suit::Spades),
        c(Rank::Four, Suit::Spades),
        c(Rank::Seven, Suit::Clubs),
        c(Rank::Three, Suit::Hearts),
    ];

    /* River flush blockers (88, 89, 91, 92, 93, 95) */
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::Two, Suit::Diamonds)],
            &three_spade_river
        ),
        BUCKET_RIVER_NUT_FLUSH_BLOCKER_AIR
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::King, Suit::Spades), c(Rank::Two, Suit::Diamonds)],
            &[
                c(Rank::Queen, Suit::Spades),
                c(Rank::Nine, Suit::Spades),
                c(Rank::Four, Suit::Spades),
                c(Rank::Seven, Suit::Clubs),
                c(Rank::Three, Suit::Hearts),
            ]
        ),
        BUCKET_RIVER_SECOND_NUT_FLUSH_BLOCKER_AIR
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::Nine, Suit::Diamonds)],
            &three_spade_river
        ),
        BUCKET_RIVER_NUT_FLUSH_BLOCKER_WEAK_PAIR
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::King, Suit::Diamonds)],
            &three_spade_river
        ),
        BUCKET_RIVER_NUT_FLUSH_BLOCKER_TOP_PAIR
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::Nine, Suit::Diamonds)],
            &[
                c(Rank::King, Suit::Spades),
                c(Rank::Nine, Suit::Spades),
                c(Rank::Four, Suit::Spades),
                c(Rank::Nine, Suit::Clubs),
                c(Rank::Three, Suit::Hearts),
            ]
        ),
        BUCKET_RIVER_NUT_FLUSH_BLOCKER_TWO_PAIR_PLUS
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::Ten, Suit::Hearts)],
            &[
                c(Rank::King, Suit::Spades),
                c(Rank::Queen, Suit::Spades),
                c(Rank::Jack, Suit::Spades),
                c(Rank::Nine, Suit::Clubs),
                c(Rank::Two, Suit::Diamonds),
            ]
        ),
        BUCKET_RIVER_STRAIGHT_NUT_FLUSH_BLOCKER
    );

    /* River straight blockers (90, 94) */
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Jack, Suit::Clubs), c(Rank::Two, Suit::Diamonds)],
            &[
                c(Rank::Ace, Suit::Spades),
                c(Rank::King, Suit::Diamonds),
                c(Rank::Queen, Suit::Hearts),
                c(Rank::Seven, Suit::Clubs),
                c(Rank::Three, Suit::Spades),
            ]
        ),
        BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_AIR
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Diamonds), c(Rank::King, Suit::Hearts)],
            &[
                c(Rank::King, Suit::Diamonds),
                c(Rank::Jack, Suit::Spades),
                c(Rank::Ten, Suit::Clubs),
                c(Rank::Seven, Suit::Spades),
                c(Rank::Two, Suit::Hearts),
            ]
        ),
        BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_TOP_PAIR
    );

    /* Monster / dangerous board splits (96, 97, 106, 107, 110, 111) */
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Clubs), c(Rank::Ace, Suit::Diamonds)],
            &[
                c(Rank::Ace, Suit::Hearts),
                c(Rank::Nine, Suit::Spades),
                c(Rank::Nine, Suit::Clubs),
                c(Rank::Four, Suit::Diamonds),
                c(Rank::Two, Suit::Spades),
            ]
        ),
        BUCKET_TOP_FULL_HOUSE
    );
    let paired_flush_board = [
        c(Rank::King, Suit::Spades),
        c(Rank::Nine, Suit::Spades),
        c(Rank::Nine, Suit::Clubs),
        c(Rank::Four, Suit::Spades),
        c(Rank::Two, Suit::Diamonds),
    ];
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::Queen, Suit::Spades)],
            &paired_flush_board
        ),
        BUCKET_PAIRED_BOARD_NUT_FLUSH
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ten, Suit::Spades), c(Rank::Eight, Suit::Spades)],
            &paired_flush_board
        ),
        BUCKET_PAIRED_BOARD_NON_NUT_FLUSH
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ten, Suit::Clubs), c(Rank::Nine, Suit::Diamonds)],
            &[
                c(Rank::Queen, Suit::Spades),
                c(Rank::Jack, Suit::Spades),
                c(Rank::Eight, Suit::Spades),
                c(Rank::Four, Suit::Clubs),
            ]
        ),
        BUCKET_THREE_FLUSH_STRAIGHT
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::King, Suit::Diamonds)],
            &[
                c(Rank::King, Suit::Hearts),
                c(Rank::Ace, Suit::Clubs),
                c(Rank::Seven, Suit::Diamonds),
                c(Rank::Two, Suit::Spades),
            ]
        ),
        BUCKET_STRICT_TOP_TWO_PAIR
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::King, Suit::Spades), c(Rank::King, Suit::Diamonds)],
            &[
                c(Rank::King, Suit::Hearts),
                c(Rank::Nine, Suit::Clubs),
                c(Rank::Four, Suit::Diamonds),
            ]
        ),
        BUCKET_TRUE_TOP_SET
    );

    /* Flop & Turn potential splits (98..=105, 108, 109) */
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Clubs), c(Rank::King, Suit::Diamonds)],
            &[
                c(Rank::Nine, Suit::Spades),
                c(Rank::Six, Suit::Hearts),
                c(Rank::Two, Suit::Diamonds),
            ]
        ),
        BUCKET_FLOP_TWO_OVERCARDS_AIR
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::Seven, Suit::Spades)],
            &[
                c(Rank::King, Suit::Spades),
                c(Rank::Seven, Suit::Diamonds),
                c(Rank::Two, Suit::Clubs),
            ]
        ),
        BUCKET_FLOP_WEAK_PAIR_BDFD
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::King, Suit::Spades)],
            &[
                c(Rank::King, Suit::Diamonds),
                c(Rank::Nine, Suit::Spades),
                c(Rank::Four, Suit::Clubs),
            ]
        ),
        BUCKET_FLOP_TOP_PAIR_BDFD
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Clubs), c(Rank::Ace, Suit::Spades)],
            &[
                c(Rank::King, Suit::Diamonds),
                c(Rank::Nine, Suit::Spades),
                c(Rank::Four, Suit::Spades),
            ]
        ),
        BUCKET_FLOP_OVERPAIR_BDFD
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::King, Suit::Spades)],
            &[
                c(Rank::Nine, Suit::Spades),
                c(Rank::Six, Suit::Spades),
                c(Rank::Two, Suit::Diamonds),
            ]
        ),
        BUCKET_NFD_TWO_OVERCARDS
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Queen, Suit::Spades), c(Rank::Jack, Suit::Spades)],
            &[
                c(Rank::Nine, Suit::Spades),
                c(Rank::Six, Suit::Spades),
                c(Rank::Two, Suit::Diamonds),
            ]
        ),
        BUCKET_FD_TWO_OVERCARDS
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Jack, Suit::Clubs), c(Rank::Nine, Suit::Hearts)],
            &[
                c(Rank::King, Suit::Spades),
                c(Rank::Queen, Suit::Diamonds),
                c(Rank::Nine, Suit::Clubs),
                c(Rank::Two, Suit::Hearts),
            ]
        ),
        BUCKET_PAIR_GUTSHOT
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Clubs), c(Rank::Jack, Suit::Diamonds)],
            &[
                c(Rank::Jack, Suit::Spades),
                c(Rank::Seven, Suit::Hearts),
                c(Rank::Two, Suit::Clubs),
            ]
        ),
        BUCKET_LOW_BOARD_TOP_PAIR
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Jack, Suit::Clubs), c(Rank::Seven, Suit::Hearts)],
            &[
                c(Rank::King, Suit::Diamonds),
                c(Rank::Ten, Suit::Spades),
                c(Rank::Nine, Suit::Diamonds),
                c(Rank::Five, Suit::Clubs),
            ]
        ),
        BUCKET_DOUBLE_GUTSHOT
    );
    assert_eq!(
        postflop_extended_bucket(
            &[c(Rank::Ace, Suit::Spades), c(Rank::King, Suit::Spades)],
            &[
                c(Rank::King, Suit::Diamonds),
                c(Rank::Nine, Suit::Spades),
                c(Rank::Four, Suit::Spades),
            ]
        ),
        BUCKET_TOP_PAIR_NUT_FD
    );
}
