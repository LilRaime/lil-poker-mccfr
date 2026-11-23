use super::{LEGAL_FULL, LEGAL_OPEN, LEGAL_SHOVE, make_overbet_bluffer, make_passive_station};
use lil_poker_mccfr::cfr::abstraction::{detect_river_nut_blocker, postflop_equity_bucket};
use lil_poker_mccfr::cfr::opponent_model::{OpponentTracker, PolicyRequest};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, Card, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, Rank, Suit,
};
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[test]
fn test_river_blocker_detection_and_bluffing() {
    let card = |rank, suit| Card { rank, suit };
    let board = [
        card(Rank::King, Suit::Spades),
        card(Rank::Nine, Suit::Spades),
        card(Rank::Four, Suit::Spades),
        card(Rank::Two, Suit::Hearts),
        card(Rank::Seven, Suit::Diamonds),
    ];

    let hole_with_nut_blocker = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Three, Suit::Clubs),
    ];
    assert!(
        detect_river_nut_blocker(&hole_with_nut_blocker, &board),
        "Ace of spades on 3-spade board must be detected as nut blocker"
    );

    let hole_no_blocker = [
        card(Rank::Queen, Suit::Clubs),
        card(Rank::Three, Suit::Clubs),
    ];
    assert!(
        !detect_river_nut_blocker(&hole_no_blocker, &board),
        "Hand without spade should not trigger nut blocker"
    );

    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(123);
    let raw_probs = [0.40, 0.40, 0.0, 0.10, 0.10, 0.0];
    let legal = [FOLD, CALL_CHECK, RAISE_THIRD_POT, RAISE_HALF_POT];
    let mut raises_with_blocker = 0;
    let mut raises_without_blocker = 0;
    for _ in 0..200 {
        let a1 = tracker.select_action(
            &PolicyRequest {
                raw_probs: &raw_probs,
                legal_actions: &legal,
                bucket: 10,
                round: 4,
                pot: 1000,
                hole: Some(&hole_with_nut_blocker),
                board: Some(&board),
                ..Default::default()
            },
            &mut rng,
        );
        if a1 == RAISE_THIRD_POT || a1 == RAISE_HALF_POT {
            raises_with_blocker += 1;
        }
        let a2 = tracker.select_action(
            &PolicyRequest {
                raw_probs: &raw_probs,
                legal_actions: &legal,
                bucket: 10,
                round: 4,
                pot: 1000,
                hole: Some(&hole_no_blocker),
                board: Some(&board),
                ..Default::default()
            },
            &mut rng,
        );
        if a2 == RAISE_THIRD_POT || a2 == RAISE_HALF_POT {
            raises_without_blocker += 1;
        }
    }
    assert!(
        raises_with_blocker > raises_without_blocker,
        "River bluffing frequency with nut blocker ({raises_with_blocker}) must exceed without blocker ({raises_without_blocker})"
    );
}

#[test]
fn test_shove_defense_wet_turn_and_river_four_straight() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);
    let call_heavy_probs = [0.05, 0.90, 0.05, 0.0, 0.0, 0.0];

    let hole_jj = [
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Clubs),
    ];
    let board_wet = [
        Card::new(Rank::Four, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Diamonds),
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Three, Suit::Clubs),
    ];
    let chosen_jj_turn = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &[FOLD, CALL_CHECK],
            bucket: 43,
            round: 3,
            to_call: 3661,
            pot: 4500,
            hole: Some(&hole_jj),
            board: Some(&board_wet),
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        chosen_jj_turn, FOLD,
        "Overpair JJ on a 4-to-straight Turn board facing all-in raise must fold!"
    );

    let board_four_straight = [
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Ace, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Hearts),
        Card::new(Rank::Queen, Suit::Spades),
        Card::new(Rank::Jack, Suit::Diamonds),
    ];
    let act_set = tracker.select_action(
        &PolicyRequest {
            raw_probs: &call_heavy_probs,
            legal_actions: &LEGAL_SHOVE,
            bucket: 68,
            round: 4,
            to_call: 6725,
            pot: 9975,
            board: Some(&board_four_straight),
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_set, FOLD,
        "Set on a 4-to-a-straight river board must fold to a river shove!"
    );
}

#[test]
fn test_shove_defense_river_overcards_and_flop_air_cap() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);

    let hole_jj = [
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Clubs),
    ];
    let board_overcards = [
        Card::new(Rank::Queen, Suit::Clubs),
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Three, Suit::Hearts),
        Card::new(Rank::Ace, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Diamonds),
    ];
    let chosen_jj_river = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &[FOLD, CALL_CHECK],
            bucket: 35,
            round: 4,
            to_call: 3254,
            pot: 4000,
            hole: Some(&hole_jj),
            board: Some(&board_overcards),
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        chosen_jj_river, FOLD,
        "Fourth pair on Q-K-3-A-6 River board facing heavy all-in call must fold!"
    );

    let station = make_passive_station();
    let act = station.select_action(
        &PolicyRequest {
            raw_probs: &[0.05, 0.05, 0.20, 0.20, 0.20, 0.30],
            legal_actions: &LEGAL_FULL,
            bucket: 6,
            round: 2,
            to_call: 250,
            pot: 500,
            ..Default::default()
        },
        &mut rng,
    );
    assert!(
        act < RAISE_MIN,
        "Pure air facing a flop bet must not bluff-raise or shove, got {act}"
    );

    /* Flop small 1 BB bet (~25% pot odds) with Ace-high overcards must not force 100% fold */
    let hole_a9 = [
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Diamonds),
    ];
    let board_q72_dry = [
        Card::new(Rank::Queen, Suit::Spades),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let b_a9 = postflop_equity_bucket(&hole_a9, &board_q72_dry);
    let act_small_stab = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.05, 0.90, 0.05, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_FULL,
            bucket: b_a9,
            round: 2,
            to_call: 100,
            pot: 300,
            hole: Some(&hole_a9),
            board: Some(&board_q72_dry),
            table_bb: 100,
            my_chips: 9000,
            ..Default::default()
        },
        &mut rng,
    );
    assert_ne!(
        act_small_stab, FOLD,
        "Ace-high two overcards facing 1 BB flop bet must not force-fold"
    );

    /* Loose sticky caller (calls and raises ~30%, low folds < 20%) suppresses pure-air flop c-bets */
    let mut sticky = OpponentTracker::new();
    for _ in 0..14 {
        sticky.record_action_street(CALL_CHECK, 2, true);
    }
    for _ in 0..6 {
        sticky.record_action_street(RAISE_MIN, 1, false);
    }
    sticky.record_action_street(FOLD, 2, true);
    let mut sticky_checks = 0usize;
    for _ in 0..80 {
        let a = sticky.select_action(
            &PolicyRequest {
                raw_probs: &[0.0, 0.15, 0.35, 0.30, 0.20, 0.0],
                legal_actions: &LEGAL_OPEN,
                bucket: 4,
                round: 2,
                to_call: 0,
                pot: 300,
                table_bb: 100,
                my_chips: 9000,
                ..Default::default()
            },
            &mut rng,
        );
        if a == CALL_CHECK {
            sticky_checks += 1;
        }
    }
    assert!(
        sticky_checks >= 60,
        "Pure air vs sticky caller on flop must check mostly (got {sticky_checks}/80 checks)"
    );
}

#[test]
fn test_postflop_barrel_folds_and_turn_pot_commit() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(2026);

    let board_flop_dry = [
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Three, Suit::Hearts),
    ];
    let act_bottom = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_FULL,
            bucket: 30,
            round: 2,
            to_call: 800,
            pot: 2000,
            board: Some(&board_flop_dry),
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_bottom, FOLD,
        "Bare bottom pair must fold to a 2/3 pot flop bet!"
    );

    let board_monotone_turn = [
        Card::new(Rank::Four, Suit::Spades),
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Eight, Suit::Spades),
        Card::new(Rank::Six, Suit::Diamonds),
    ];
    let act_tp_mono = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_FULL,
            bucket: 42,
            round: 3,
            to_call: 2900,
            pot: 7250,
            board: Some(&board_monotone_turn),
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_tp_mono, FOLD,
        "Bare top pair mid kicker without flush redraw on 3-flush turn must fold to heavy barrel!"
    );

    let board_turn_committed = [
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Three, Suit::Diamonds),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Five, Suit::Diamonds),
    ];
    let act_committed = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.95, 0.05, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_SHOVE,
            bucket: 58,
            round: 3,
            to_call: 600,
            pot: 9400,
            board: Some(&board_turn_committed),
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_ne!(
        act_committed, FOLD,
        "Calling 600 into 9400 (6% pot odds) with overpair must never fold!"
    );

    /* Flop 3-bet pot underpair below 2 overcards facing large barrel must fold */
    let hole_55 = [
        Card::new(Rank::Five, Suit::Clubs),
        Card::new(Rank::Five, Suit::Spades),
    ];
    let board_at2 = [
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::Ten, Suit::Clubs),
        Card::new(Rank::Two, Suit::Hearts),
    ];
    let b_55 = postflop_equity_bucket(&hole_55, &board_at2);
    let act_underpair_3bp = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.05, 0.95, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_FULL,
            bucket: b_55,
            round: 2,
            to_call: 2206,
            pot: 5206,
            hole: Some(&hole_55),
            board: Some(&board_at2),
            table_bb: 200,
            my_chips: 6682,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_underpair_3bp, FOLD,
        "Underpair 55 under A-T in bloated 3-bet pot must fold to 11 BB flop barrel"
    );
}

#[test]
fn test_monotone_flop_and_paired_river_postflop_guards() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(2027);
    let raise_heavy = [0.05, 0.25, 0.20, 0.20, 0.15, 0.15];
    let call_heavy = [0.05, 0.90, 0.05, 0.0, 0.0, 0.0];

    let board_monotone_flop = [
        Card::new(Rank::Jack, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Clubs),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let act_oesd = tracker.select_action(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: 16,
            round: 2,
            to_call: 400,
            pot: 800,
            board: Some(&board_monotone_flop),
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(act_oesd, FOLD, "Non-flush OESD on 3-flush flop must fold!");

    let board_dry_flop = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Two, Suit::Spades),
        Card::new(Rank::Nine, Suit::Clubs),
    ];
    let act_tp_flop = tracker.select_action(
        &PolicyRequest {
            raw_probs: &raise_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: 46,
            round: 2,
            to_call: 500,
            pot: 1000,
            board: Some(&board_dry_flop),
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_tp_flop, CALL_CHECK,
        "Bare Top Pair vs flop donk must flat-call!"
    );

    let board_paired_river = [
        Card::new(Rank::Four, Suit::Diamonds),
        Card::new(Rank::Eight, Suit::Clubs),
        Card::new(Rank::Six, Suit::Hearts),
        Card::new(Rank::Two, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Spades),
    ];
    let act_trips = tracker.select_action(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_SHOVE,
            bucket: 65,
            round: 4,
            to_call: 9562,
            pot: 13124,
            board: Some(&board_paired_river),
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_trips, FOLD,
        "Weak trips on paired river must fold to 48 BB shove!"
    );
}

#[test]
fn test_priority_tier_ordering_and_decision_trace_reasons() {
    let station = make_passive_station();
    let mut rng = SmallRng::seed_from_u64(999);
    let passive_probs = [0.0, 0.85, 0.05, 0.05, 0.05, 0.0];
    let board_paired_flush = [
        Card::new(Rank::Five, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Eight, Suit::Hearts),
    ];
    let (act, trace) = station.select_action_traced(
        &PolicyRequest {
            raw_probs: &passive_probs,
            legal_actions: &LEGAL_OPEN,
            bucket: 76,
            round: 4,
            pot: 2000,
            board: Some(&board_paired_flush),
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert!(
        act != ALL_IN && act != RAISE_HALF_POT,
        "Priority Tier 2 cap must override Tier 1 passive-station boost, got {act}"
    );
    assert!(
        trace.guard_reasons.contains(&"PASSIVE_STATION_BOOST")
            && trace
                .guard_reasons
                .contains(&"POST_PAIRED_FLUSH_STRAIGHT_CAP"),
        "DecisionTrace must record both Tier 1 and Tier 2 reasons: {:?}",
        trace.guard_reasons
    );
}

#[test]
fn test_river_set_overbet_cap_on_dangerous_board() {
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
    let bucket = postflop_equity_bucket(&hole_set, &board_3straight);
    assert!(
        (68..=72).contains(&bucket),
        "Set on 3-straight river board should fall in set bucket 68..=72, got {bucket}"
    );

    let policy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.15, 0.15, 0.0, 0.30, 0.40];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN, RAISE_HALF_POT, ALL_IN];
    let mut rng = SmallRng::seed_from_u64(4242);
    for _ in 0..100 {
        let (act, trace) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &policy,
                legal_actions: &legal,
                bucket,
                round: 4,
                to_call: 1200,
                pot: 1500,
                hole: Some(&hole_set),
                board: Some(&board_3straight),
                table_bb: 200,
                my_chips: 10000,
                ..Default::default()
            },
            &mut rng,
        );
        assert_eq!(
            act, CALL_CHECK,
            "Set facing a 6 BB river bet on dangerous 3-straight board must flat-call (reasons: {:?})",
            trace.guard_reasons
        );
    }
}

#[test]
fn test_queen_high_flush_no_fold_and_monotone_j_fd() {
    let mut tracker = OpponentTracker::new();
    for _ in 0..10 {
        tracker.record_action_street_detailed(RAISE_HALF_POT, 1, false, 100, 300);
        tracker.end_hand();
    }
    let mut rng = SmallRng::seed_from_u64(42);

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
    let (act_flush, trace_flush) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &[0.85, 0.15, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &[FOLD, CALL_CHECK],
            bucket: b_q_flush,
            round: 3,
            to_call: 8140,
            pot: 3200,
            hole: Some(&hole_q_flush),
            board: Some(&board_3flush_turn),
            my_chips: 8660,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_flush, CALL_CHECK,
        "Queen-high flush on unpaired 3-flush turn must call shove (reasons: {:?})",
        trace_flush.guard_reasons
    );

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
        "Kd Jh must retain Flush Draw bucket >= 19, got {b_j_fd}"
    );
    let (act_fd, _) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &[0.05, 0.85, 0.10, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_FULL,
            bucket: b_j_fd,
            round: 2,
            to_call: 400,
            pot: 800,
            hole: Some(&hole_j_fd),
            board: Some(&board_mono_flop),
            table_bb: 150,
            my_chips: 8375,
            ..Default::default()
        },
        &mut rng,
    );
    assert_ne!(
        act_fd, FOLD,
        "Jack-high FD + overs on monotone flop must not fold"
    );
}

#[test]
fn test_pair_plus_nfd_turn_no_fold_and_low_spr_river_nut_blocker() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_116);

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
    assert_eq!(b_26, 26);
    let mut turn_calls = 0;
    for _ in 0..60 {
        let (act_turn, trace_turn) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &[0.25, 0.70, 0.05, 0.0, 0.0, 0.0],
                legal_actions: &[FOLD, CALL_CHECK, RAISE_MIN, ALL_IN],
                bucket: b_26,
                round: 3,
                to_call: 2400,
                pot: 4000,
                hole: Some(&hole_as8c),
                board: Some(&board_turn_3spades),
                table_bb: 200,
                my_chips: 4318,
                ..Default::default()
            },
            &mut rng,
        );
        assert!(
            !trace_turn.guard_reasons.iter().any(|&r| {
                r == "SHOVE_DEFENSE_FOLD"
                    || r == "TURN_DRAW_ODDS_FOLD"
                    || r == "POST_BARREL_FOLD"
                    || r == "RANGE_EQUITY_SHOVE_FOLD"
            }),
            "Pair + NFD (bucket 26) triggered fold guard on 3-flush Turn (reasons: {:?})",
            trace_turn.guard_reasons
        );
        if act_turn == CALL_CHECK {
            turn_calls += 1;
        }
    }
    assert!(
        turn_calls >= 35,
        "Pair + NFD (bucket 26) must call predominantly on 3-flush Turn (got {turn_calls}/60)"
    );

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
    let check_heavy = [0.0, 0.85, 0.05, 0.10, 0.0, 0.0];
    let mut blocker_bluffs = 0usize;
    let mut fired_reason = false;
    for _ in 0..80 {
        let (act_river, tr) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &check_heavy,
                legal_actions: &LEGAL_OPEN,
                bucket: b_air,
                round: 4,
                pot: 6878,
                hole: Some(&hole_ahkc),
                board: Some(&board_river_3hearts),
                table_bb: 150,
                my_chips: 3619,
                ..Default::default()
            },
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
        "Low-SPR river with Ace nut blocker must bluff sometimes (got {blocker_bluffs}/80)"
    );
}

#[test]
fn test_monster_value_raise_and_river_halfpot_tp() {
    let mut rng = SmallRng::seed_from_u64(4242);
    let tracker = OpponentTracker::new();
    let raise_heavy = [0.0, 0.05, 0.0, 0.0, 0.95, 0.0];
    let mut raises = 0usize;
    for _ in 0..50 {
        let act = tracker.select_action(
            &PolicyRequest {
                raw_probs: &raise_heavy,
                legal_actions: &LEGAL_FULL,
                bucket: 85,
                round: 4,
                to_call: 500,
                pot: 2000,
                ..Default::default()
            },
            &mut rng,
        );
        if act >= RAISE_MIN {
            raises += 1;
        }
    }
    assert!(
        raises >= 40,
        "Monster bucket 85 must keep value-raising (got {raises}/50)"
    );

    let board_dry_river = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Ten, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Clubs),
        Card::new(Rank::Three, Suit::Hearts),
        Card::new(Rank::Two, Suit::Spades),
    ];
    let act_tp = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.05, 0.95, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_FULL,
            bucket: 47,
            round: 4,
            to_call: 500,
            pot: 1500,
            board: Some(&board_dry_river),
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_tp, CALL_CHECK,
        "Top Pair must call standard 5 BB half-pot river bet"
    );
}

#[test]
fn test_flop_commit_draw_fold() {
    let mut rng = SmallRng::seed_from_u64(4242);
    let tracker = OpponentTracker::new();

    let board_fd_flop = [
        Card::new(Rank::Four, Suit::Clubs),
        Card::new(Rank::Eight, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Spades),
    ];
    let hole_k5c = [
        Card::new(Rank::King, Suit::Clubs),
        Card::new(Rank::Five, Suit::Clubs),
    ];
    let (act_fd_commit, _) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_FULL,
            bucket: 21,
            round: 2,
            to_call: 2250,
            pot: 3750,
            hole: Some(&hole_k5c),
            board: Some(&board_fd_flop),
            table_bb: 300,
            my_chips: 4712,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_fd_commit, FOLD,
        "Bare flush draw facing 1.5x pot flop overbet committing 48% of stack must fold"
    );

    /* Bare OESD on deep stack facing 1.5x pot flop overbet (37.5% pot odds > 34%) must fold */
    let hole_q9 = [
        Card::new(Rank::Queen, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Spades),
    ];
    let board_j2t_flop = [
        Card::new(Rank::Jack, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Spades),
        Card::new(Rank::Ten, Suit::Clubs),
    ];
    let b_q9 = postflop_equity_bucket(&hole_q9, &board_j2t_flop);
    let (act_oesd_overbet, tr_oesd) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_FULL,
            bucket: b_q9,
            round: 2,
            to_call: 1200,
            pot: 2000,
            hole: Some(&hole_q9),
            board: Some(&board_j2t_flop),
            table_bb: 150,
            my_chips: 13500,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_oesd_overbet, FOLD,
        "Bare OESD on deep stack facing 1.5x pot flop overbet must fold (reasons: {:?})",
        tr_oesd.guard_reasons
    );

    /* Bare draw in bloated 3-bet flop pot must cap oversized c-bet sizing (no half/all-in) */
    let hole_56h = [
        Card::new(Rank::Five, Suit::Hearts),
        Card::new(Rank::Six, Suit::Hearts),
    ];
    let board_8a7_flop = [
        Card::new(Rank::Eight, Suit::Clubs),
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Clubs),
    ];
    let b_56h = postflop_equity_bucket(&hole_56h, &board_8a7_flop);
    let half_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.15, 0.05, 0.15, 0.55, 0.10];
    for _ in 0..40 {
        let (act_draw_cbet, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &half_heavy,
                legal_actions: &LEGAL_OPEN,
                bucket: b_56h,
                round: 2,
                pot: 2202,
                hole: Some(&hole_56h),
                board: Some(&board_8a7_flop),
                table_bb: 150,
                my_chips: 11500,
                ..Default::default()
            },
            &mut rng,
        );
        assert!(
            act_draw_cbet != RAISE_HALF_POT && act_draw_cbet != ALL_IN,
            "Bare draw in 3-bet flop pot must not fire oversized 75%+ c-bet (got {act_draw_cbet})"
        );
    }
}

#[test]
fn test_flop_wet_set_value_floor() {
    let mut rng = SmallRng::seed_from_u64(20_261_119);
    let tracker = OpponentTracker::new();
    let passive_check: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.85, 0.0, 0.15, 0.0, 0.0];
    let hole_jj = [
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Spades),
    ];
    let board_wet_set_flop = [
        Card::new(Rank::Five, Suit::Clubs),
        Card::new(Rank::Three, Suit::Diamonds),
        Card::new(Rank::Jack, Suit::Clubs),
    ];
    let b_wet_set = postflop_equity_bucket(&hole_jj, &board_wet_set_flop);
    let mut wet_set_bets = 0usize;
    for _ in 0..80 {
        let (act, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &passive_check,
                legal_actions: &LEGAL_OPEN,
                bucket: b_wet_set,
                round: 2,
                pot: 1000,
                hole: Some(&hole_jj),
                board: Some(&board_wet_set_flop),
                table_bb: 200,
                my_chips: 5331,
                ..Default::default()
            },
            &mut rng,
        );
        if act >= RAISE_MIN {
            wet_set_bets += 1;
        }
    }
    assert!(
        wet_set_bets >= 45,
        "Top Set on wet flop must value-bet majority of the time, got {wet_set_bets}/80"
    );

    /* Ultra-low SPR (0.34) overpair on flop must jam/value-bet instead of checking down */
    let hole_88 = [
        Card::new(Rank::Eight, Suit::Diamonds),
        Card::new(Rank::Eight, Suit::Clubs),
    ];
    let board_low_spr_flop = [
        Card::new(Rank::Three, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Four, Suit::Spades),
    ];
    let b_88 = postflop_equity_bucket(&hole_88, &board_low_spr_flop);
    let mut low_spr_bets = 0usize;
    for _ in 0..80 {
        let (act, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &passive_check,
                legal_actions: &LEGAL_OPEN,
                bucket: b_88,
                round: 2,
                pot: 9480,
                hole: Some(&hole_88),
                board: Some(&board_low_spr_flop),
                table_bb: 300,
                my_chips: 3208,
                ..Default::default()
            },
            &mut rng,
        );
        if act >= RAISE_MIN {
            low_spr_bets += 1;
        }
    }
    assert!(
        low_spr_bets >= 50,
        "Overpair 88 at SPR 0.34 on flop must push value mostly (got {low_spr_bets}/80)"
    );
}

#[test]
fn test_flop_floats_overbet_bluff_catch_and_polarized_sizing() {
    let mut rng = SmallRng::seed_from_u64(20_261_123);
    let neutral = OpponentTracker::new();
    let overbet_villain = make_overbet_bluffer();
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.10, 0.90, 0.0, 0.0, 0.0, 0.0];

    /* 1. Single-raised pot flop floats (50%–60% pot, 2.5–3.0 BB) must NOT auto-fold */
    let hole_ako = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::King, Suit::Clubs),
    ];
    let board_99t = [
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Ten, Suit::Spades),
    ];
    let hole_j5 = [
        Card::new(Rank::Five, Suit::Clubs),
        Card::new(Rank::Jack, Suit::Hearts),
    ];
    let board_986 = [
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Eight, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Clubs),
    ];
    let hole_kj = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Jack, Suit::Hearts),
    ];
    let board_764 = [
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Six, Suit::Spades),
        Card::new(Rank::Four, Suit::Hearts),
    ];
    for (hole, board, to_call, pot, bb) in [
        (hole_ako, board_99t, 900, 2400, 300),
        (hole_j5, board_986, 1200, 3200, 400),
        (hole_kj, board_764, 375, 1125, 150),
    ] {
        let b = postflop_equity_bucket(&hole, &board);
        let (act, tr) = neutral.select_action_traced(
            &PolicyRequest {
                raw_probs: &call_heavy,
                legal_actions: &LEGAL_FULL,
                bucket: b,
                round: 2,
                to_call,
                pot,
                hole: Some(&hole),
                board: Some(&board),
                table_bb: bb,
                my_chips: bb * 20,
                ..Default::default()
            },
            &mut rng,
        );
        assert_ne!(
            act, FOLD,
            "Single-raised flop float {hole:?} (bucket {b}) must not force-fold (reasons: {:?})",
            tr.guard_reasons
        );
    }

    /* 2. Flop OVERBET_BLUFF_CATCH vs 150% overbet bluffer: bottom pair (K2s), underpair (22), and OESD (Q9o) */
    let fold_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.85, 0.15, 0.0, 0.0, 0.0, 0.0];
    let hole_k2s = [
        Card::new(Rank::Two, Suit::Spades),
        Card::new(Rank::King, Suit::Spades),
    ];
    let board_298 = [
        Card::new(Rank::Two, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Eight, Suit::Hearts),
    ];
    let hole_q9 = [
        Card::new(Rank::Queen, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Spades),
    ];
    let board_jt4 = [
        Card::new(Rank::Jack, Suit::Spades),
        Card::new(Rank::Ten, Suit::Diamonds),
        Card::new(Rank::Four, Suit::Diamonds),
    ];
    for (hole, board) in [(hole_k2s, board_298), (hole_q9, board_jt4)] {
        let b = postflop_equity_bucket(&hole, &board);
        let mut calls = 0usize;
        for _ in 0..80 {
            let (act, _) = overbet_villain.select_action_traced(
                &PolicyRequest {
                    raw_probs: &fold_heavy,
                    legal_actions: &LEGAL_FULL,
                    bucket: b,
                    round: 2,
                    to_call: 1200,
                    pot: 2000,
                    hole: Some(&hole),
                    board: Some(&board),
                    table_bb: 150,
                    my_chips: 6000,
                    ..Default::default()
                },
                &mut rng,
            );
            if act == CALL_CHECK {
                calls += 1;
            }
        }
        assert!(
            calls >= 40,
            "Flop bluff-catcher/OESD {hole:?} vs overbet bluffer must call often (got {calls}/80)"
        );
    }

    /* 3. Flop TPTK / TopPair value check-raise unlock & Turn polarized RAISE_HALF_POT sizing */
    let hole_ak = [
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::King, Suit::Hearts),
    ];
    let board_a83 = [
        Card::new(Rank::Ace, Suit::Diamonds),
        Card::new(Rank::Eight, Suit::Spades),
        Card::new(Rank::Three, Suit::Clubs),
    ];
    let b_ak = postflop_equity_bucket(&hole_ak, &board_a83);
    let mut xrs = 0usize;
    for _ in 0..80 {
        let (act, _) = overbet_villain.select_action_traced(
            &PolicyRequest {
                raw_probs: &[0.0, 0.85, 0.05, 0.10, 0.0, 0.0],
                legal_actions: &LEGAL_FULL,
                bucket: b_ak,
                round: 2,
                to_call: 400,
                pot: 1200,
                hole: Some(&hole_ak),
                board: Some(&board_a83),
                table_bb: 100,
                my_chips: 9000,
                ..Default::default()
            },
            &mut rng,
        );
        if act >= RAISE_MIN {
            xrs += 1;
        }
    }
    assert!(
        xrs >= 15,
        "TPTK on dry flop vs aggro opponent must check-raise sometimes (got {xrs}/80)"
    );

    let board_turn_a834 = [
        Card::new(Rank::Ace, Suit::Diamonds),
        Card::new(Rank::Eight, Suit::Spades),
        Card::new(Rank::Three, Suit::Clubs),
        Card::new(Rank::Four, Suit::Hearts),
    ];
    let mut polarized_bets = 0usize;
    for _ in 0..80 {
        let (act, _) = neutral.select_action_traced(
            &PolicyRequest {
                raw_probs: &[0.0, 0.45, 0.35, 0.10, 0.10, 0.0],
                legal_actions: &LEGAL_OPEN,
                bucket: 70,
                round: 3,
                pot: 1000,
                hole: Some(&hole_ak),
                board: Some(&board_turn_a834),
                table_bb: 100,
                my_chips: 8000,
                ..Default::default()
            },
            &mut rng,
        );
        if act == RAISE_HALF_POT {
            polarized_bets += 1;
        }
    }
    assert!(
        polarized_bets >= 20,
        "Set on dry Turn must shift weight into polarized RAISE_HALF_POT sizing (got {polarized_bets}/80)"
    );

    /* 4. Normal HU opponent (Fold ~20%, Flop FTC 50%) must not suppress IP Flop c-bets */
    let mut normal_hu = OpponentTracker::new();
    for _ in 0..10 {
        normal_hu.record_action_street(CALL_CHECK, 2, false);
    }
    for _ in 0..6 {
        normal_hu.record_action_street(RAISE_MIN, 2, false);
    }
    for _ in 0..3 {
        normal_hu.record_action_street(FOLD, 2, true);
    }
    for _ in 0..3 {
        normal_hu.record_action_street(CALL_CHECK, 2, true);
    }
    let mut cbets = 0usize;
    for _ in 0..80 {
        let (act, _) = normal_hu.select_action_traced(
            &PolicyRequest {
                raw_probs: &[0.0, 0.35, 0.65, 0.0, 0.0, 0.0],
                legal_actions: &LEGAL_OPEN,
                bucket: 3,
                round: 2,
                pot: 500,
                active_opponents: 1,
                table_bb: 100,
                my_chips: 9700,
                ..Default::default()
            },
            &mut rng,
        );
        if act >= RAISE_MIN {
            cbets += 1;
        }
    }
    assert!(
        cbets >= 25,
        "Normal HU opponent with 50% flop FTC must not trigger sticky-caller c-bet suppression (got {cbets}/80)"
    );

    /* 5. Deep Flop TPTK facing a 3-bet must cap ALL_IN */
    let hole_khah = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::King, Suit::Hearts),
    ];
    let board_k8t = [
        Card::new(Rank::King, Suit::Clubs),
        Card::new(Rank::Eight, Suit::Diamonds),
        Card::new(Rank::Ten, Suit::Clubs),
    ];
    let b_tptk = postflop_equity_bucket(&hole_khah, &board_k8t);
    let mut tptk_jams = 0usize;
    for _ in 0..80 {
        let (act, _) = neutral.select_action_traced(
            &PolicyRequest {
                raw_probs: &[0.0, 0.50, 0.10, 0.05, 0.05, 0.30],
                legal_actions: &LEGAL_FULL,
                bucket: b_tptk,
                round: 2,
                to_call: 595,
                pot: 2575,
                hole: Some(&hole_khah),
                board: Some(&board_k8t),
                active_opponents: 1,
                table_bb: 100,
                my_chips: 9010,
                ..Default::default()
            },
            &mut rng,
        );
        if act == ALL_IN {
            tptk_jams += 1;
        }
    }
    assert_eq!(
        tptk_jams, 0,
        "Deep Flop TPTK facing 3-bet must cap ALL_IN (got {tptk_jams}/80)"
    );

    /* 6. Clean two-overcard Flop float facing a 43% pot c-bet (pot_odds = 0.30 <= 0.31) */
    let hole_k9 = [
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Clubs),
    ];
    let board_852 = [
        Card::new(Rank::Eight, Suit::Diamonds),
        Card::new(Rank::Five, Suit::Spades),
        Card::new(Rank::Two, Suit::Hearts),
    ];
    let b_k9 = postflop_equity_bucket(&hole_k9, &board_852);
    let mut k9_calls = 0usize;
    for _ in 0..80 {
        let (act, _) = neutral.select_action_traced(
            &PolicyRequest {
                raw_probs: &[0.85, 0.15, 0.0, 0.0, 0.0, 0.0],
                legal_actions: &LEGAL_FULL,
                bucket: b_k9,
                round: 2,
                to_call: 300,
                pot: 700,
                hole: Some(&hole_k9),
                board: Some(&board_852),
                active_opponents: 1,
                table_bb: 100,
                my_chips: 9700,
                ..Default::default()
            },
            &mut rng,
        );
        if act == CALL_CHECK {
            k9_calls += 1;
        }
    }
    assert!(
        k9_calls >= 8,
        "Clean two-overcard Flop float vs 43% pot c-bet must rescue calls (got {k9_calls}/80)"
    );
}
