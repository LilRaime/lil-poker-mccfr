use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
use lil_poker_mccfr::game::holdem::{
    Card, Rank, Suit, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use rand::SeedableRng;

#[test]
fn test_river_blocker_detection_and_bluffing() {
    use lil_poker_mccfr::cfr::abstraction::detect_river_nut_blocker;

    let card = |rank, suit| Card { rank, suit };
    /* Board with 3 Spades (Ks, 9s, 4s, 2h, 7d) */
    let board = [
        card(Rank::King, Suit::Spades),
        card(Rank::Nine, Suit::Spades),
        card(Rank::Four, Suit::Spades),
        card(Rank::Two, Suit::Hearts),
        card(Rank::Seven, Suit::Diamonds),
    ];

    /* Hole has Ace of spades -> holding Nut Flush blocker */
    let hole_with_nut_blocker = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Three, Suit::Clubs),
    ];
    assert!(
        detect_river_nut_blocker(&hole_with_nut_blocker, &board),
        "Ace of spades on 3-spade board must be detected as nut blocker"
    );

    /* Hole without Ace of spades -> no blocker */
    let hole_no_blocker = [
        card(Rank::Queen, Suit::Clubs),
        card(Rank::Three, Suit::Clubs),
    ];
    assert!(
        !detect_river_nut_blocker(&hole_no_blocker, &board),
        "Hand without spade should not trigger nut blocker"
    );

    /* Verify purified selection boosts bluff with blocker */
    let tracker = OpponentTracker::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(123);
    let raw_probs = [0.40, 0.40, 0.0, 0.10, 0.10, 0.0];
    let legal = [FOLD, CALL_CHECK, RAISE_THIRD_POT, RAISE_HALF_POT];

    /* Sample 200 actions with blocker vs without blocker */
    let mut raises_with_blocker = 0;
    let mut raises_without_blocker = 0;
    for _ in 0..200 {
        let a1 = tracker.select_action_purified_context(
            &raw_probs,
            &legal,
            10,
            4,
            0,
            1000,
            Some(&hole_with_nut_blocker),
            Some(&board),
            &mut rng,
        );
        if a1 == RAISE_THIRD_POT || a1 == RAISE_HALF_POT {
            raises_with_blocker += 1;
        }

        let a2 = tracker.select_action_purified_context(
            &raw_probs,
            &legal,
            10,
            4,
            0,
            1000,
            Some(&hole_no_blocker),
            Some(&board),
            &mut rng,
        );
        if a2 == RAISE_THIRD_POT || a2 == RAISE_HALF_POT {
            raises_without_blocker += 1;
        }
    }

    assert!(
        raises_with_blocker > raises_without_blocker,
        "River bluffing frequency with nut blocker ({}) must exceed without blocker ({})",
        raises_with_blocker,
        raises_without_blocker
    );
}

#[test]
fn test_shove_defense_pocket_pairs_and_board_textures() {
    let tracker = OpponentTracker::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);
    let card = |rank, suit| Card { rank, suit };
    let legal = [FOLD, CALL_CHECK];

    /* 1. Pocket 88 (bucket 6) facing 40 BB preflop shove (to_call = 4000)
     * Even if raw CFR probs suggests calling with some weight,
     * the guard MUST classify 88 as trash against a massive 35+ BB shove and fold! */
    let raw_probs_88 = [0.20, 0.80, 0.0, 0.0, 0.0, 0.0];
    let chosen_88 = tracker.select_action_purified_context(
        &raw_probs_88,
        &legal,
        6, /* 88 */
        1, /* Preflop */
        4000,
        5000,
        None,
        None,
        &mut rng,
    );
    assert_eq!(chosen_88, FOLD, "Pocket 88 facing a 40 BB shove must fold!");

    /* 2. Monster Pocket AA (bucket 0) facing 40 BB preflop shove
     * Must NEVER fold */
    let raw_probs_aa = [0.40, 0.60, 0.0, 0.0, 0.0, 0.0];
    let chosen_aa = tracker.select_action_purified_context(
        &raw_probs_aa,
        &legal,
        0, /* AA */
        1, /* Preflop */
        4000,
        5000,
        None,
        None,
        &mut rng,
    );
    assert_eq!(chosen_aa, CALL_CHECK, "Pocket Aces facing shove must call!");

    /* 3. Overpair JJ on a 4-to-straight Turn board (4d 6d 7c 3c) facing 35+ BB check-raise shove
     * Board has 3, 4, 6, 7 (any 5 is straight). JJ is bare overpair (bucket 43).
     * Must fold against shove on dangerous board! */
    let hole_jj = [
        card(Rank::Jack, Suit::Hearts),
        card(Rank::Jack, Suit::Clubs),
    ];
    let board_wet = [
        card(Rank::Four, Suit::Diamonds),
        card(Rank::Six, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Three, Suit::Clubs),
    ];
    let raw_probs_jj = [0.10, 0.90, 0.0, 0.0, 0.0, 0.0];
    let chosen_jj_turn = tracker.select_action_purified_context(
        &raw_probs_jj,
        &legal,
        43, /* Mid Overpair */
        3,  /* Turn */
        3661,
        4500,
        Some(&hole_jj),
        Some(&board_wet),
        &mut rng,
    );
    assert_eq!(
        chosen_jj_turn, FOLD,
        "Overpair JJ on a 4-to-straight Turn board facing all-in raise must fold!"
    );

    /* 4. Fourth Pair JJ on Q-K-3-A-6 River board facing heavy bet / all-in
     * (bucket 35, to_call = 3254, cannot raise). Must fold! */
    let board_overcards = [
        card(Rank::Queen, Suit::Clubs),
        card(Rank::King, Suit::Spades),
        card(Rank::Three, Suit::Hearts),
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Six, Suit::Diamonds),
    ];
    let chosen_jj_river = tracker.select_action_purified_context(
        &raw_probs_jj,
        &legal,
        35, /* Middle Pair under multiple overcards */
        4,  /* River */
        3254,
        4000,
        Some(&hole_jj),
        Some(&board_overcards),
        &mut rng,
    );
    assert_eq!(
        chosen_jj_river, FOLD,
        "Fourth pair on Q-K-3-A-6 River board facing heavy all-in call must fold!"
    );
}

#[test]
fn test_persistent_opponent_profile_and_bluff_spew_guard() {
    let mut tracker = OpponentTracker::new();
    for _ in 0..10 {
        tracker.record_action_street_detailed(CALL_CHECK, 1, false, 100, 200);
        /* Duplicate preflop action in same hand must not double-count vpip_hands */
        tracker.record_action_street_detailed(CALL_CHECK, 1, false, 200, 400);
        tracker.end_hand();
    }
    assert_eq!(tracker.total_hands, 10);
    assert_eq!(tracker.vpip_hands, 10);
    assert!((tracker.vpip_ratio() - 1.0).abs() < 1e-6);

    let tmp_path = "/tmp/lil_poker_test_opp_profiles.json";
    tracker
        .save_profile(tmp_path, "test_villain")
        .expect("save profile");
    let loaded =
        OpponentTracker::load_profile(tmp_path, "test_villain").expect("load saved profile");
    assert_eq!(loaded.total_hands, 9); /* 10 * 0.90 session decay */
    let _ = std::fs::remove_file(tmp_path);

    /* Bluff-spew guard: facing a bet on Flop with pure air (bucket 6), never check-raise or shove */
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);
    let legal = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let raw_probs = [0.05, 0.05, 0.20, 0.20, 0.20, 0.30];
    let act = tracker.select_action_purified_context(
        &raw_probs, &legal, 6, /* Queen-high pure air */
        2, /* Flop */
        250, 500, None, None, &mut rng,
    );
    assert!(
        act < RAISE_MIN,
        "Pure air facing a flop bet must not bluff-raise or shove, got {}",
        act
    );
}

#[test]
fn test_postflop_barrel_and_deep_shove_defense() {
    let mut maniac = OpponentTracker::new();
    for _ in 0..12 {
        maniac.record_action_street_detailed(ALL_IN, 1, true, 4000, 300);
        maniac.end_hand();
    }
    let mut rng = rand::rngs::SmallRng::seed_from_u64(2026);

    /* 1. Offsuit QJo (bucket 77, Tier 4) facing a 29 BB cold preflop shove (5800 into 6200, bb=200)
     * must FOLD even against a known maniac! */
    let legal_shove = [FOLD, CALL_CHECK, ALL_IN];
    let call_heavy_probs = [0.05, 0.90, 0.05, 0.0, 0.0, 0.0];
    let act_qjo_shove = maniac.select_action_purified_context_full(
        &call_heavy_probs,
        &legal_shove,
        77, /* QJo */
        1,
        5800,
        6200,
        None,
        None,
        1,
        200,
        &mut rng,
    );
    assert_eq!(
        act_qjo_shove, FOLD,
        "Offsuit QJo must fold to a 29 BB preflop shove!"
    );

    /* 2. Pocket 77 (bucket 8) facing a standard 2x open (to_call = 400, bb = 200)
     * must NOT 3-bet all-in jam! */
    let tracker = OpponentTracker::new();
    let legal_full = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let shove_probs = [0.0, 0.10, 0.0, 0.0, 0.0, 0.90];
    let act_mid_pair_open = tracker.select_action_purified_context_full(
        &shove_probs,
        &legal_full,
        8, /* 77 */
        1,
        400,
        1000,
        None,
        None,
        1,
        200,
        &mut rng,
    );
    assert_ne!(
        act_mid_pair_open, ALL_IN,
        "Pocket 77 facing a standard 2x open must not 3-bet jam ALL_IN!"
    );

    /* 3. Set of Queens (bucket 68) on a 4-to-a-straight River (Kh Ad 2h Qs Jd)
     * facing a river shove (6725 into 9975) must FOLD! */
    let board_four_straight = [
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Ace, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Hearts),
        Card::new(Rank::Queen, Suit::Spades),
        Card::new(Rank::Jack, Suit::Diamonds),
    ];
    let act_set_four_straight = tracker.select_action_purified_context_full(
        &call_heavy_probs,
        &legal_shove,
        68, /* Middle Set QQ */
        4,
        6725,
        9975,
        None,
        Some(&board_four_straight),
        1,
        200,
        &mut rng,
    );
    assert_eq!(
        act_set_four_straight, FOLD,
        "Set on a 4-to-a-straight river board must fold to a river shove!"
    );

    /* 4. Straight (bucket 76) on a paired + 3-flush River (5h 9s 7d 9h 8h)
     * must NOT overbet (RAISE_HALF_POT) or shove (ALL_IN)! */
    let board_paired_flush = [
        Card::new(Rank::Five, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Eight, Suit::Hearts),
    ];
    let legal_no_fold = [
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let jam_probs = [0.0, 0.05, 0.0, 0.05, 0.45, 0.45];
    let act_straight_paired = tracker.select_action_purified_context_full(
        &jam_probs,
        &legal_no_fold,
        76, /* Straight */
        4,
        0,
        1750,
        None,
        Some(&board_paired_flush),
        1,
        200,
        &mut rng,
    );
    assert!(
        act_straight_paired != ALL_IN && act_straight_paired != RAISE_HALF_POT,
        "Straight on paired 3-flush river must not jam or 125% overbet, got {}",
        act_straight_paired
    );

    /* 5. Bottom pair without draw (bucket 30) on 9h 7c 3h facing a 2/3 pot Flop bet (800 into 2000) must FOLD! */
    let board_flop_dry = [
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Three, Suit::Hearts),
    ];
    let act_bottom_pair_flop = tracker.select_action_purified_context_full(
        &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
        &legal_full,
        30,
        2,
        800,
        2000,
        None,
        Some(&board_flop_dry),
        1,
        200,
        &mut rng,
    );
    assert_eq!(
        act_bottom_pair_flop, FOLD,
        "Bare bottom pair must fold to a 2/3 pot flop bet!"
    );

    /* 6. Vulnerable Top Pair Mid Kicker without flush redraw (bucket 42)
     * on monotone 4s Ks 8s 6d Turn facing 2900 into 7250 must FOLD! */
    let board_monotone_turn = [
        Card::new(Rank::Four, Suit::Spades),
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Eight, Suit::Spades),
        Card::new(Rank::Six, Suit::Diamonds),
    ];
    let act_tp_monotone = tracker.select_action_purified_context_full(
        &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
        &legal_full,
        42,
        3,
        2900,
        7250,
        None,
        Some(&board_monotone_turn),
        1,
        200,
        &mut rng,
    );
    assert_eq!(
        act_tp_monotone, FOLD,
        "Bare top pair mid kicker without flush redraw on 3-flush turn must fold to heavy barrel!"
    );

    /* 7. Pot-committed Turn call (600 into 9400 on 3-flush turn, ~6% pot odds) must NEVER fold! */
    let board_turn_committed = [
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Three, Suit::Diamonds),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Five, Suit::Diamonds),
    ];
    let act_committed_turn = tracker.select_action_purified_context_full(
        &[0.95, 0.05, 0.0, 0.0, 0.0, 0.0],
        &legal_shove,
        58, /* Two Pair / Overpair above board pair */
        3,
        600,
        9400,
        None,
        Some(&board_turn_committed),
        1,
        200,
        &mut rng,
    );
    assert_ne!(
        act_committed_turn, FOLD,
        "Calling 600 into 9400 (6% pot odds) with overpair must never fold!"
    );
}

#[test]
fn test_3bet_4bet_defense_and_monotone_board_guards() {
    let mut maniac = OpponentTracker::new();
    for _ in 0..12 {
        maniac.record_preflop_positional(true, true, false);
        maniac.record_action_street_detailed(ALL_IN, 1, true, 4000, 300);
        maniac.end_hand();
    }
    let mut rng = rand::rngs::SmallRng::seed_from_u64(2027);
    let legal_full = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let legal_shove = [FOLD, CALL_CHECK, ALL_IN];

    /* 1. Small pocket pair 55 (bucket 10, Tier 4) facing a preflop 3-bet (to_call = 500, pot = 1500, bb = 200)
     * must flat call (or fold), NEVER 4-bet raise or 4-bet jam even if opponent has a high SB limp ratio! */
    let raise_heavy = [0.05, 0.25, 0.20, 0.20, 0.15, 0.15];
    let act_55_vs_3bet = maniac.select_action_purified_context_full(
        &raise_heavy,
        &legal_full,
        10, /* 55 */
        1,
        500,
        1500,
        None,
        None,
        1,
        200,
        &mut rng,
    );
    assert!(
        act_55_vs_3bet < RAISE_MIN,
        "Pocket 55 facing a preflop 3-bet must not 4-bet bluff or jam, got {}",
        act_55_vs_3bet
    );

    /* 2. Suited Ace A8s (bucket 34, Tier 3) facing a 32 BB preflop shove (6400 into 6800, bb = 200)
     * must FOLD even against a known maniac! */
    let call_heavy = [0.05, 0.90, 0.05, 0.0, 0.0, 0.0];
    let act_a8s_shove = maniac.select_action_purified_context_full(
        &call_heavy,
        &legal_shove,
        34, /* A8s */
        1,
        6400,
        6800,
        None,
        None,
        1,
        200,
        &mut rng,
    );
    assert_eq!(
        act_a8s_shove, FOLD,
        "Suited A8s must fold to a 32 BB preflop shove even against an aggressive opponent!"
    );

    /* 3. Suited one-gapper 97s (bucket 51, Tier 5) facing a 13.3 BB preflop shove (2663 into 3063, bb = 200)
     * must FOLD even against a known maniac! */
    let act_97s_shove = maniac.select_action_purified_context_full(
        &call_heavy,
        &legal_shove,
        51, /* 97s */
        1,
        2663,
        3063,
        None,
        None,
        1,
        200,
        &mut rng,
    );
    assert_eq!(
        act_97s_shove, FOLD,
        "Suited 97s must fold to a 13.3 BB preflop shove!"
    );

    /* 4. Non-flush OESD (bucket 16) on a monotone 3-club Flop (Jc 9c 2c) facing a half-pot bet (400 into 800) must FOLD! */
    let tracker = OpponentTracker::new();
    let board_monotone_flop = [
        Card::new(Rank::Jack, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Clubs),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let act_oesd_monotone = tracker.select_action_purified_context_full(
        &call_heavy,
        &legal_full,
        16, /* Non-flush OESD */
        2,
        400,
        800,
        None,
        Some(&board_monotone_flop),
        1,
        200,
        &mut rng,
    );
    assert_eq!(
        act_oesd_monotone, FOLD,
        "Non-flush OESD on a monotone 3-flush flop must fold to a half-pot bet!"
    );

    /* 5. Bare Top Pair Good Kicker (bucket 46) on Flop facing a half-pot donk-bet (500 into 1000)
     * must flat call rather than raising! */
    let board_dry_flop = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Two, Suit::Spades),
        Card::new(Rank::Nine, Suit::Clubs),
    ];
    let act_tp_flop = tracker.select_action_purified_context_full(
        &raise_heavy,
        &legal_full,
        46, /* Top Pair Good Kicker */
        2,
        500,
        1000,
        None,
        Some(&board_dry_flop),
        1,
        200,
        &mut rng,
    );
    assert_eq!(
        act_tp_flop, CALL_CHECK,
        "Bare Top Pair facing a half-pot flop bet must flat-call rather than raise!"
    );

    /* 6. Weak Trips (bucket 65) on a paired River board (4d 8c 6h 2d 6s) facing a 48 BB river shove
     * (9562 into 13124, bb = 200) must FOLD! */
    let board_paired_river = [
        Card::new(Rank::Four, Suit::Diamonds),
        Card::new(Rank::Eight, Suit::Clubs),
        Card::new(Rank::Six, Suit::Hearts),
        Card::new(Rank::Two, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Spades),
    ];
    let act_weak_trips_river = tracker.select_action_purified_context_full(
        &call_heavy,
        &legal_shove,
        65, /* Weak Trips */
        4,
        9562,
        13124,
        None,
        Some(&board_paired_river),
        1,
        200,
        &mut rng,
    );
    assert_eq!(
        act_weak_trips_river, FOLD,
        "Weak trips on a paired river board must fold to a 48 BB river shove!"
    );
}

#[test]
fn test_priority_tier_ordering_and_decision_trace_reasons() {
    use lil_poker_mccfr::cfr::subgame::OpponentReachDistribution;
    use lil_poker_mccfr::game::holdem::ALL_52_CARDS;

    /* 1. Verify Priority Tier 2/3 hard caps override Priority Tier 1 passive-station boosts */
    let mut station = OpponentTracker::new();
    for _ in 0..15 {
        station.record_action_street(CALL_CHECK, 1, false);
        station.record_action_street(CALL_CHECK, 2, false);
        station.end_hand();
    }
    let mut rng = rand::rngs::SmallRng::seed_from_u64(999);
    let legal_no_fold = [
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let passive_probs = [0.0, 0.85, 0.05, 0.05, 0.05, 0.0];

    /* Straight (bucket 76) on a paired 3-flush river against a calling station:
     * Priority Tier 1 boosts value sizing, but Priority Tier 2 must still block ALL_IN and RAISE_HALF_POT! */
    let board_paired_flush = [
        Card::new(Rank::Five, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Eight, Suit::Hearts),
    ];
    let (act, trace) = station.select_action_purified_traced(
        &passive_probs,
        &legal_no_fold,
        76,
        4,
        0,
        2000,
        None,
        Some(&board_paired_flush),
        1,
        200,
        &mut rng,
    );
    assert!(
        act != ALL_IN && act != RAISE_HALF_POT,
        "Priority Tier 2 paired/flush river cap must override Priority Tier 1 passive-station boost, got {}",
        act
    );
    assert!(
        trace.guard_reasons.contains(&"PASSIVE_STATION_BOOST")
            && trace
                .guard_reasons
                .contains(&"POST_PAIRED_FLUSH_STRAIGHT_CAP"),
        "DecisionTrace must record both Tier 1 and Tier 2 guard reasons: {:?}",
        trace.guard_reasons
    );

    /* 2. Verify VPIP-adapted Bayesian range narrowing in OpponentReachDistribution */
    let board_flop = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let hole_bot = [
        Card::new(Rank::Queen, Suit::Diamonds),
        Card::new(Rank::Jack, Suit::Diamonds),
    ];
    let remaining_deck: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|c| !board_flop.contains(c) && !hole_bot.contains(c))
        .collect();
    let history = [vec![RAISE_HALF_POT, CALL_CHECK], vec![], vec![], vec![]];

    let dist_nit = OpponentReachDistribution::build_with_stats(
        &remaining_deck,
        &board_flop,
        2,
        &history,
        None,
        Some(0),
        Some(0.15),
        Some(0.12),
    );
    let dist_loose = OpponentReachDistribution::build_with_stats(
        &remaining_deck,
        &board_flop,
        2,
        &history,
        None,
        Some(0),
        Some(0.75),
        Some(0.10),
    );

    /* Trash hand 72o should have higher normalized probability in a 75% VPIP range than in a 15% VPIP nit range */
    let c7h = Card::new(Rank::Seven, Suit::Hearts);
    let c2d = Card::new(Rank::Two, Suit::Diamonds);
    let find_prob = |dist: &OpponentReachDistribution| -> f64 {
        dist.combos
            .iter()
            .zip(dist.weights.iter())
            .find(|(c, _)| (c[0] == c7h && c[1] == c2d) || (c[0] == c2d && c[1] == c7h))
            .map(|(_, &w)| w / dist.total_weight.max(1e-12))
            .unwrap_or(0.0)
    };
    let p_nit = find_prob(&dist_nit);
    let p_loose = find_prob(&dist_loose);
    assert!(
        p_loose > p_nit,
        "Loose VPIP opponent must assign higher relative reach probability to weak offsuit combos ({:.6} vs {:.6})",
        p_loose,
        p_nit
    );
}

#[test]
fn test_river_set_overbet_cap_on_dangerous_board() {
    use lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket;
    use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;

    let tracker = OpponentTracker::new();
    let hole_set = [
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Clubs),
    ];
    let board_3straight = [
        Card::new(Rank::Jack, Suit::Clubs),
        Card::new(Rank::Ten, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Four, Suit::Hearts),
        Card::new(Rank::Two, Suit::Spades),
    ];
    let bucket = postflop_equity_bucket(&hole_set, &board_3straight) as usize;
    assert!(
        (68..=72).contains(&bucket),
        "Set on 3-straight river board should fall in set bucket 68..=72, got {}",
        bucket
    );

    let policy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.15, 0.15, 0.0, 0.30, 0.40];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN, RAISE_HALF_POT, ALL_IN];
    let mut rng = rand::thread_rng();
    for _ in 0..100 {
        let (act, trace) = tracker.select_action_purified_traced_with_stack(
            &policy,
            &legal,
            bucket,
            4,
            1200,
            1500,
            Some(&hole_set),
            Some(&board_3straight),
            1,
            200,
            10000,
            &mut rng,
        );
        assert_eq!(
            act, CALL_CHECK,
            "Set facing a 6 BB river bet on a dangerous 3-straight board must flat-call instead of raising/shoving (reasons: {:?})",
            trace.guard_reasons
        );
    }
}

#[test]
fn test_queen_high_flush_no_fold_sb_no_limp_fold_and_monotone_j_fd() {
    use lil_poker_mccfr::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};

    let mut tracker = OpponentTracker::new();
    for _ in 0..10 {
        tracker.record_action_street_detailed(RAISE_HALF_POT, 1, false, 100, 300);
        tracker.end_hand();
    }
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);

    /* 1. Qs Ts on 7s Td 5s 3s Turn facing 8140 into 3200 must NEVER fold */
    let hole_q_flush = [
        Card::new(Rank::Queen, Suit::Spades),
        Card::new(Rank::Ten, Suit::Spades),
    ];
    let board_3flush_turn = [
        Card::new(Rank::Seven, Suit::Spades),
        Card::new(Rank::Ten, Suit::Diamonds),
        Card::new(Rank::Five, Suit::Spades),
        Card::new(Rank::Three, Suit::Spades),
    ];
    let b_q_flush = postflop_equity_bucket(&hole_q_flush, &board_3flush_turn);
    assert_eq!(b_q_flush, 81);
    let legal_turn_shove = [FOLD, CALL_CHECK];
    let raw_heavy_fold = [0.85, 0.15, 0.0, 0.0, 0.0, 0.0];
    let (act_flush, trace_flush) = tracker.select_action_purified_traced_with_stack(
        &raw_heavy_fold,
        &legal_turn_shove,
        b_q_flush,
        3,
        8140,
        3200,
        Some(&hole_q_flush),
        Some(&board_3flush_turn),
        1,
        100,
        8660,
        &mut rng,
    );
    assert_eq!(
        act_flush, CALL_CHECK,
        "Queen-high flush on unpaired 3-flush turn must call shove (reasons: {:?})",
        trace_flush.guard_reasons
    );

    /* 2. Kd Jh on Th 7h 4h monotone flop counts as real Flush Draw (>= 19) and calls 50% pot c-bet */
    let hole_j_fd = [
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Jack, Suit::Hearts),
    ];
    let board_mono_flop = [
        Card::new(Rank::Ten, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Four, Suit::Hearts),
    ];
    let b_j_fd = postflop_equity_bucket(&hole_j_fd, &board_mono_flop);
    assert!(
        b_j_fd >= 19,
        "Kd Jh on Th 7h 4h must retain Flush Draw bucket >= 19, got {}",
        b_j_fd
    );
    let legal_flop = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let raw_call = [0.05, 0.85, 0.10, 0.0, 0.0, 0.0];
    let (act_fd, _) = tracker.select_action_purified_traced_with_stack(
        &raw_call,
        &legal_flop,
        b_j_fd,
        2,
        400,
        800,
        Some(&hole_j_fd),
        Some(&board_mono_flop),
        1,
        150,
        8375,
        &mut rng,
    );
    assert_ne!(
        act_fd, FOLD,
        "Jack-high flush draw + two overs on monotone flop must not fold to half-pot c-bet"
    );

    /* 3. Tier 7 (Th 7c) on 100 BB HU SB must never limp */
    let b_t7o = preflop_bucket(
        Card::new(Rank::Ten, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Clubs),
    )
    .0 as usize;
    let raw_limp = [0.10, 0.80, 0.10, 0.0, 0.0, 0.0];
    for _ in 0..20 {
        let (act_sb, _) = tracker.select_action_purified_traced_with_stack(
            &raw_limp,
            &legal_flop,
            b_t7o,
            1,
            50,
            150,
            None,
            None,
            1,
            100,
            9950,
            &mut rng,
        );
        assert_ne!(
            act_sb, CALL_CHECK,
            "Tier 7 T7o on 100 BB HU SB must fold or steal-raise, never limp-fold"
        );
    }
}

#[test]
fn test_pair_plus_nfd_turn_no_fold_and_low_spr_river_nut_blocker() {
    use lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket;

    let tracker = OpponentTracker::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(20261116);

    /* 1. Turn Pair + Nut Flush Draw (As 8c on [5c 8s Ks 6s], bucket 26)
     * facing a 1.5x pot turn overbet (2400 into 4000, pot_odds = 37.5%, bb = 200) must not fold! */
    let hole_as8c = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Eight, Suit::Clubs),
    ];
    let board_turn_3spades = [
        Card::new(Rank::Five, Suit::Clubs),
        Card::new(Rank::Eight, Suit::Spades),
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Six, Suit::Spades),
    ];
    let b_26 = postflop_equity_bucket(&hole_as8c, &board_turn_3spades);
    assert_eq!(
        b_26, 26,
        "As 8c on 5c 8s Ks 6s must be bucket 26 (Pair + NFD)"
    );

    let legal_turn = [FOLD, CALL_CHECK, RAISE_MIN, ALL_IN];
    let call_heavy = [0.25, 0.70, 0.05, 0.0, 0.0, 0.0];
    let (act_turn, trace_turn) = tracker.select_action_purified_traced_with_stack(
        &call_heavy,
        &legal_turn,
        b_26,
        3,
        2400,
        4000,
        Some(&hole_as8c),
        Some(&board_turn_3spades),
        1,
        200,
        4318,
        &mut rng,
    );
    assert_ne!(
        act_turn, FOLD,
        "Pair + Nut Flush Draw (14 outs, bucket 26) must not fold on 3-flush Turn (reasons: {:?})",
        trace_turn.guard_reasons
    );

    /* 2. River Ace nut-flush blocker (Ah Kc on [3h 2h 7h Td Jc]) at low SPR
     * (my_chips = 3619, pot = 6878, bb = 150) must still allow RIVER_NUT_BLOCKER_BLUFF! */
    let hole_ahkc = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::King, Suit::Clubs),
    ];
    let board_river_3hearts = [
        Card::new(Rank::Three, Suit::Hearts),
        Card::new(Rank::Two, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Ten, Suit::Diamonds),
        Card::new(Rank::Jack, Suit::Clubs),
    ];
    let b_air = postflop_equity_bucket(&hole_ahkc, &board_river_3hearts);
    let legal_river = [
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let check_heavy = [0.0, 0.85, 0.05, 0.10, 0.0, 0.0];
    let mut blocker_bluffs = 0usize;
    let mut fired_reason = false;
    for _ in 0..80 {
        let (act_river, tr) = tracker.select_action_purified_traced_with_stack(
            &check_heavy,
            &legal_river,
            b_air,
            4,
            0,
            6878,
            Some(&hole_ahkc),
            Some(&board_river_3hearts),
            1,
            150,
            3619,
            &mut rng,
        );
        if act_river >= RAISE_MIN {
            blocker_bluffs += 1;
        }
        if tr.guard_reasons.contains(&"RIVER_NUT_BLOCKER_BLUFF") {
            fired_reason = true;
        }
    }
    assert!(
        fired_reason && blocker_bluffs >= 10,
        "Low-SPR river with Ace nut-flush blocker must trigger RIVER_NUT_BLOCKER_BLUFF and bluff sometimes (got {}/80)",
        blocker_bluffs
    );
}
