use super::{card, cards, hole};
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
    BUCKET_TOP_PAIR_NUT_FD, BUCKET_TRUE_TOP_SET, BoardFlushTexture, calculate_range_equity,
    detect_board_texture, detect_draws, get_holdem_infoset_key, nash_call_shove_max_bb,
    nash_open_push_max_bb, postflop_equity_bucket, postflop_extended_bucket, preflop_bucket,
};

#[test]
fn test_draw_detection_and_equity_buckets() {
    let (hole_fd, board_fd) = (hole("As 4s"), cards("Ks 8s 2d"));
    let (is_fd, is_sd) = detect_draws(&hole_fd, &board_fd);
    assert!(is_fd, "Should detect flush draw");
    assert!(!is_sd, "Should not detect straight draw");
    let bucket_fd = postflop_equity_bucket(&hole_fd, &board_fd);
    assert!(
        bucket_fd >= 15,
        "Flush draw with Ace should be high tier draw (bucket >= 15), got {bucket_fd}"
    );

    /* One Pair AA vs One Pair 22 must not map to the same bucket */
    let bucket_aa = postflop_equity_bucket(&hole("Ah 2c"), &cards("As 7d 9c"));
    let bucket_22 = postflop_equity_bucket(&hole("2h 3c"), &cards("2s 7d 9c"));
    assert!(
        bucket_aa > bucket_22,
        "Pair of Aces ({bucket_aa}) must be in higher bucket than Pair of Twos ({bucket_22})"
    );
}

#[test]
fn test_postflop_pair_hierarchy_and_board_pairs() {
    let bucket_qq = postflop_equity_bucket(&hole("Qs Qh"), &cards("Td 7c 2h"));
    assert_eq!(
        bucket_qq, 54,
        "Dry Overpair QQ on T-7-2 should be bucket 54"
    );

    let board_a = cards("Ad 7c 2h");
    let bucket_ak = postflop_equity_bucket(&hole("As Kh"), &board_a);
    let bucket_a3 = postflop_equity_bucket(&hole("As 3h"), &board_a);
    assert_eq!(bucket_ak, 47, "Dry TPTK AK on A-7-2 should be bucket 47");
    assert_eq!(
        bucket_a3, 44,
        "Dry Top pair weak kicker should be bucket 44"
    );
    assert!(
        bucket_ak > bucket_a3,
        "TPTK must be strictly higher than weak kicker top pair"
    );

    /* Board Pair: 7-2 on A-A-K must be unpaired high card (<= 10), not Top Pair Aces */
    let bucket_board_pair = postflop_equity_bucket(&hole("7s 2h"), &cards("Ad Ac Kh"));
    assert!(
        bucket_board_pair <= 10,
        "7-2 on A-A-K board must be evaluated as unpaired high card (<= 10, got {bucket_board_pair})"
    );
}

#[test]
fn test_board_texture_and_redraw_potential() {
    let tex_mono = detect_board_texture(&cards("Kh 8h 4h"));
    assert_eq!(tex_mono.flush_texture, BoardFlushTexture::Monotone);
    assert!(!tex_mono.is_paired);

    let tex_pair = detect_board_texture(&cards("9s 9h 8d"));
    assert!(tex_pair.is_paired);
    assert!(tex_pair.is_connected);

    let board_two_tone = cards("Ad 7h 2h");
    let bucket_with_fd = postflop_equity_bucket(&hole("Ah Kh"), &board_two_tone);
    let bucket_dry = postflop_equity_bucket(&hole("As Kc"), &board_two_tone);
    assert!(
        bucket_with_fd > bucket_dry,
        "TPTK with Nut Flush Draw ({bucket_with_fd}) must have higher bucket than dry TPTK ({bucket_dry})"
    );
}

#[test]
fn test_kicker_aware_flush_and_straight_abstraction() {
    let board_mono = cards("As Js Ts 3s 2s");
    for (h, expected, msg) in [
        (
            "Ks 4d",
            83,
            "King of spades on Ace-board must be Nut Flush (bucket 83)",
        ),
        (
            "Qs 4d",
            82,
            "Queen of spades on Ace-board must be 2nd Nut Flush (bucket 82)",
        ),
        (
            "6s 4s",
            79,
            "6s 4s on Ace-board must be Low Flush (bucket 79)",
        ),
        (
            "7h 8d",
            79,
            "No spades in hand must be Board Flush (bucket 79)",
        ),
    ] {
        assert_eq!(
            postflop_equity_bucket(&hole(h), &board_mono),
            expected,
            "{msg}"
        );
    }

    let board_straight = cards("5s 6h 7d 8c 9s");
    let b_nut_st = postflop_equity_bucket(&hole("Ts 2c"), &board_straight);
    let b_board_st = postflop_equity_bucket(&hole("2h 3c"), &board_straight);
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
    let board_paired = cards("8d 8c 2h");
    for (h, expected, desc) in [
        (
            "As Ah",
            52,
            "AA on 8-8-2 must be bucket 52 (Vulnerable High Overpair on paired board)",
        ),
        (
            "5s 5h",
            37,
            "55 on 8-8-2 must be bucket 37 (Pocket pair below 1 board overcard rank)",
        ),
        (
            "Ks Kh",
            52,
            "KK on 8-8-2 must be bucket 52 (Vulnerable High Overpair on paired board)",
        ),
    ] {
        let got = postflop_equity_bucket(&hole(h), &board_paired);
        assert_eq!(got, expected, "{desc}, got {got}");
    }
}

#[test]
fn test_range_equity_and_nash_short_stack_thresholds() {
    let hole_tp = hole("As Jd");
    let board_river = cards("Ac 9h 4s 2d 7c");
    let eq_vs_loose = calculate_range_equity(&hole_tp, &board_river, 0.70, false, 1);
    let eq_vs_tight_bet = calculate_range_equity(&hole_tp, &board_river, 0.20, true, 1);
    assert!(
        eq_vs_loose > eq_vs_tight_bet,
        "Equity against wide passive range ({eq_vs_loose:.3}) must exceed equity against tight aggressive range ({eq_vs_tight_bet:.3})"
    );

    let eq_multiway = calculate_range_equity(&hole_tp, &board_river, 0.50, false, 3);
    assert!(
        eq_multiway < eq_vs_loose,
        "Multi-way equity ({eq_multiway:.3}) must be lower than heads-up equity ({eq_vs_loose:.3})"
    );

    let (aa_idx, _) = preflop_bucket(card("As"), card("Ah"));
    let (a5s_idx, _) = preflop_bucket(card("As"), card("5s"));
    let (trash_idx, _) = preflop_bucket(card("7s"), card("2h"));

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
    /* 1. Flop Nut Backdoor FD + Overcard -> weak draw (11..=14), Turn brick -> High Card (<= 10) */
    let hole_bdfd = hole("Ah Td");
    let b_flop = postflop_equity_bucket(&hole_bdfd, &cards("Qs 7h 2h"));
    assert!(
        (11..=14).contains(&b_flop),
        "Flop Nut Backdoor FD + Overcard must be in weak draw range 11..=14, got {b_flop}"
    );
    let b_turn = postflop_equity_bucket(&hole_bdfd, &cards("Qs 7h 2h 4c"));
    assert!(
        b_turn <= 10,
        "Bricked backdoor draw on Turn must revert to High Card (<= 10), got {b_turn}"
    );

    /* 2. Strict separation between Visible Trips (65..=67) and Disguised Sets (68..=72) */
    let b_trips = postflop_equity_bucket(&hole("9c Ad"), &cards("9s 9h 4d"));
    let b_set = postflop_equity_bucket(&hole("4c 4s"), &cards("Ks 9h 4d"));
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
    let board_quads = cards("7s 7h 7d 7c 2s");
    let weak_quad_bucket = postflop_equity_bucket(&hole("5h 4d"), &board_quads);
    let ace_quad_bucket = postflop_equity_bucket(&hole("Ah Kd"), &board_quads);
    assert!(
        weak_quad_bucket < 15,
        "Board quads with weak 5-4 kicker must be trash bucket < 15, got {weak_quad_bucket}"
    );
    assert!(
        ace_quad_bucket >= 50,
        "Board quads with Ace-high kicker must remain strong >= 50, got {ace_quad_bucket}"
    );

    let fd_bucket = postflop_equity_bucket(&hole("As Ks"), &cards("9s 9d 4s 4c"));
    assert_eq!(
        fd_bucket, 23,
        "Nut flush draw on double-paired board must preserve FD bucket 23"
    );
}

#[test]
fn test_full_house_pair_rank_and_counterfeited_pocket_pair_buckets() {
    let board_trips = cards("Ks Kh Kd 7c 2s");
    assert_eq!(
        postflop_equity_bucket(&hole("Ac Ad"), &board_trips),
        85,
        "AA on KKK72 must be top Full House (bucket 85)"
    );
    assert_eq!(
        postflop_equity_bucket(&hole("3c 3d"), &board_trips),
        84,
        "33 on KKK72 is an underfull (3 < 7) -> bucket 84"
    );

    let b_22 = postflop_equity_bucket(&hole("2c 2h"), &cards("Ks Kh Qd Qc 7s"));
    assert!(
        b_22 <= 10,
        "Counterfeited pocket pair 22 on KKQQ7 must be bucket <= 10, got {b_22}"
    );
}

#[test]
fn test_two_pair_bottom_pair_on_board_abstraction() {
    let (h, b) = (hole("As Kh"), cards("Ad 7c 2h Ts 2c"));
    assert_eq!(
        postflop_equity_bucket(&h, &b),
        43,
        "Top Pair Top Kicker on paired board must map to bucket 43, not Two Pair (63)"
    );
    let river_history = [vec![2u8, 1u8], vec![4u8, 1u8], vec![1u8, 1u8], vec![3u8]];
    assert_eq!(get_holdem_infoset_key(&h, &b, 4, &river_history), "R:B43/t");
}

#[test]
fn test_extended_blocker_and_potential_buckets() {
    const THREE_SPADE: &str = "Ks 9s 4s 7c 3h";
    const PAIRED_FLUSH: &str = "Ks 9s 9c 4s 2d";

    for (h, b, expected) in [
        /* River flush blockers (88, 89, 91, 92, 93, 95) */
        ("As 2d", THREE_SPADE, BUCKET_RIVER_NUT_FLUSH_BLOCKER_AIR),
        (
            "Ks 2d",
            "Qs 9s 4s 7c 3h",
            BUCKET_RIVER_SECOND_NUT_FLUSH_BLOCKER_AIR,
        ),
        (
            "As 9d",
            THREE_SPADE,
            BUCKET_RIVER_NUT_FLUSH_BLOCKER_WEAK_PAIR,
        ),
        (
            "As Kd",
            THREE_SPADE,
            BUCKET_RIVER_NUT_FLUSH_BLOCKER_TOP_PAIR,
        ),
        (
            "As 9d",
            "Ks 9s 4s 9c 3h",
            BUCKET_RIVER_NUT_FLUSH_BLOCKER_TWO_PAIR_PLUS,
        ),
        (
            "As Th",
            "Ks Qs Js 9c 2d",
            BUCKET_RIVER_STRAIGHT_NUT_FLUSH_BLOCKER,
        ),
        /* River straight blockers (90, 94) */
        (
            "Jc 2d",
            "As Kd Qh 7c 3s",
            BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_AIR,
        ),
        (
            "Ad Kh",
            "Kd Js Tc 7s 2h",
            BUCKET_RIVER_NUT_STRAIGHT_BLOCKER_TOP_PAIR,
        ),
        /* Monster / dangerous board splits (96, 97, 106, 107, 110, 111) */
        ("Ac Ad", "Ah 9s 9c 4d 2s", BUCKET_TOP_FULL_HOUSE),
        ("As Qs", PAIRED_FLUSH, BUCKET_PAIRED_BOARD_NUT_FLUSH),
        ("Ts 8s", PAIRED_FLUSH, BUCKET_PAIRED_BOARD_NON_NUT_FLUSH),
        ("Tc 9d", "Qs Js 8s 4c", BUCKET_THREE_FLUSH_STRAIGHT),
        ("As Kd", "Kh Ac 7d 2s", BUCKET_STRICT_TOP_TWO_PAIR),
        ("Ks Kd", "Kh 9c 4d", BUCKET_TRUE_TOP_SET),
        /* Flop & Turn potential splits (98..=105, 108, 109) */
        ("Ac Kd", "9s 6h 2d", BUCKET_FLOP_TWO_OVERCARDS_AIR),
        ("As 7s", "Ks 7d 2c", BUCKET_FLOP_WEAK_PAIR_BDFD),
        ("As Ks", "Kd 9s 4c", BUCKET_FLOP_TOP_PAIR_BDFD),
        ("Ac As", "Kd 9s 4s", BUCKET_FLOP_OVERPAIR_BDFD),
        ("As Ks", "9s 6s 2d", BUCKET_NFD_TWO_OVERCARDS),
        ("Qs Js", "9s 6s 2d", BUCKET_FD_TWO_OVERCARDS),
        ("Jc 9h", "Ks Qd 9c 2h", BUCKET_PAIR_GUTSHOT),
        ("Ac Jd", "Js 7h 2c", BUCKET_LOW_BOARD_TOP_PAIR),
        ("Jc 7h", "Kd Ts 9d 5c", BUCKET_DOUBLE_GUTSHOT),
        ("As Ks", "Kd 9s 4s", BUCKET_TOP_PAIR_NUT_FD),
    ] {
        assert_eq!(
            postflop_extended_bucket(&hole(h), &cards(b)),
            expected,
            "{h} on {b}"
        );
    }
}
