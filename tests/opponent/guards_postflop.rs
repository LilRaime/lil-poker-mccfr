use lil_poker_mccfr::cfr::abstraction::{
    detect_river_nut_blocker, postflop_equity_bucket, preflop_bucket,
};
use lil_poker_mccfr::cfr::opponent_model::{ExploitRequest, OpponentTracker, PolicyRequest};
use lil_poker_mccfr::cfr::subgame::{OpponentReachDistribution, ReachRequest};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{
    Card, Rank, Suit, ALL_52_CARDS, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN,
    RAISE_THIRD_POT,
};
use rand::rngs::SmallRng;
use rand::SeedableRng;

const LEGAL_FULL: [u8; 6] = [
    FOLD,
    CALL_CHECK,
    RAISE_MIN,
    RAISE_THIRD_POT,
    RAISE_HALF_POT,
    ALL_IN,
];
const LEGAL_SHOVE: [u8; 3] = [FOLD, CALL_CHECK, ALL_IN];

fn make_maniac_tracker() -> OpponentTracker {
    let mut maniac = OpponentTracker::new();
    for _ in 0..12 {
        maniac.record_action_street_detailed(ALL_IN, 1, true, 4000, 300);
        maniac.end_hand();
    }
    maniac
}

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
fn test_shove_defense_preflop_pairs_and_wet_turn() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);
    let legal = [FOLD, CALL_CHECK];

    let chosen_88 = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.20, 0.80, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &legal,
            bucket: 6,
            round: 1,
            to_call: 4000,
            pot: 5000,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(chosen_88, FOLD, "Pocket 88 facing a 40 BB shove must fold!");

    let chosen_aa = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.40, 0.60, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &legal,
            bucket: 0,
            round: 1,
            to_call: 4000,
            pot: 5000,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(chosen_aa, CALL_CHECK, "Pocket Aces facing shove must call!");

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
            legal_actions: &legal,
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
}

#[test]
fn test_shove_defense_river_overcards_and_profile_persistence() {
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

    let mut persisted = OpponentTracker::new();
    for _ in 0..10 {
        persisted.record_action_street_detailed(CALL_CHECK, 1, false, 100, 200);
        persisted.record_action_street_detailed(CALL_CHECK, 1, false, 200, 400);
        persisted.end_hand();
    }
    assert_eq!(persisted.total_hands, 10);
    assert_eq!(persisted.vpip_hands, 10);
    assert!((persisted.vpip_ratio() - 1.0).abs() < 1e-6);

    let tmp_path = "/tmp/lil_poker_test_opp_profiles.json";
    persisted
        .save_profile(tmp_path, "test_villain")
        .expect("save profile");
    let loaded =
        OpponentTracker::load_profile(tmp_path, "test_villain").expect("load saved profile");
    assert_eq!(loaded.total_hands, 9);
    let _ = std::fs::remove_file(tmp_path);

    let act = persisted.select_action(
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
}

#[test]
fn test_deep_shove_defense_and_paired_board_caps() {
    let maniac = make_maniac_tracker();
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(2026);
    let call_heavy_probs = [0.05, 0.90, 0.05, 0.0, 0.0, 0.0];

    let act_qjo = maniac.select_action(
        &PolicyRequest {
            raw_probs: &call_heavy_probs,
            legal_actions: &LEGAL_SHOVE,
            bucket: 77,
            round: 1,
            to_call: 5800,
            pot: 6200,
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_qjo, FOLD,
        "Offsuit QJo must fold to a 29 BB preflop shove!"
    );

    let act_77 = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.0, 0.10, 0.0, 0.0, 0.0, 0.90],
            legal_actions: &LEGAL_FULL,
            bucket: 8,
            round: 1,
            to_call: 400,
            pot: 1000,
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_ne!(
        act_77, ALL_IN,
        "Pocket 77 facing a standard 2x open must not 3-bet jam ALL_IN!"
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

    let board_paired_flush = [
        Card::new(Rank::Five, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Eight, Suit::Hearts),
    ];
    let act_straight = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.0, 0.05, 0.0, 0.05, 0.45, 0.45],
            legal_actions: &[
                CALL_CHECK,
                RAISE_MIN,
                RAISE_THIRD_POT,
                RAISE_HALF_POT,
                ALL_IN,
            ],
            bucket: 76,
            round: 4,
            pot: 1750,
            board: Some(&board_paired_flush),
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert!(
        act_straight != ALL_IN && act_straight != RAISE_HALF_POT,
        "Straight on paired 3-flush river must not jam or 125% overbet, got {act_straight}"
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
}

#[test]
fn test_maniac_preflop_3bet_4bet_and_shove_defense() {
    let mut maniac = OpponentTracker::new();
    for _ in 0..12 {
        maniac.record_preflop_positional(true, true, false);
        maniac.record_action_street_detailed(ALL_IN, 1, true, 4000, 300);
        maniac.end_hand();
    }
    let mut rng = SmallRng::seed_from_u64(2027);
    let raise_heavy = [0.05, 0.25, 0.20, 0.20, 0.15, 0.15];
    let call_heavy = [0.05, 0.90, 0.05, 0.0, 0.0, 0.0];

    let act_55 = maniac.select_action(
        &PolicyRequest {
            raw_probs: &raise_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: 10,
            round: 1,
            to_call: 500,
            pot: 1500,
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert!(
        act_55 < RAISE_MIN,
        "Pocket 55 vs 3-bet must not 4-bet, got {act_55}"
    );

    let act_a8s = maniac.select_action(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_SHOVE,
            bucket: 34,
            round: 1,
            to_call: 6400,
            pot: 6800,
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_a8s, FOLD,
        "Suited A8s must fold to 32 BB preflop shove!"
    );

    let act_97s = maniac.select_action(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_SHOVE,
            bucket: 51,
            round: 1,
            to_call: 2663,
            pot: 3063,
            table_bb: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_97s, FOLD,
        "Suited 97s must fold to 13.3 BB preflop shove!"
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
    let mut station = OpponentTracker::new();
    for _ in 0..15 {
        station.record_action_street(CALL_CHECK, 1, false);
        station.record_action_street(CALL_CHECK, 2, false);
        station.end_hand();
    }
    let mut rng = SmallRng::seed_from_u64(999);
    let legal_no_fold = [
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
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
            legal_actions: &legal_no_fold,
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
fn test_vpip_adapted_bayesian_reach_narrowing() {
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

    let dist_nit = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining_deck,
            board: &board_flop,
            round: 2,
            target_player: Some(1),
            active_player: Some(0),
            opp_vpip: Some(0.15),
            opp_pfr: Some(0.12),
        },
        &history,
    );
    let dist_loose = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining_deck,
            board: &board_flop,
            round: 2,
            target_player: Some(1),
            active_player: Some(0),
            opp_vpip: Some(0.75),
            opp_pfr: Some(0.10),
        },
        &history,
    );

    let c7h = Card::new(Rank::Seven, Suit::Hearts);
    let c2d = Card::new(Rank::Two, Suit::Diamonds);
    let find_prob = |dist: &OpponentReachDistribution| -> f64 {
        dist.combos
            .iter()
            .zip(dist.weights.iter())
            .find(|(c, _)| (c[0] == c7h && c[1] == c2d) || (c[0] == c2d && c[1] == c7h))
            .map_or(0.0, |(_, &w)| w / dist.total_weight.max(1e-12))
    };
    let p_nit = find_prob(&dist_nit);
    let p_loose = find_prob(&dist_loose);
    assert!(
        p_loose > p_nit,
        "Loose VPIP opponent must assign higher relative reach probability to weak offsuit combos ({p_loose:.6} vs {p_nit:.6})"
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
    let mut rng = rand::thread_rng();
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
fn test_queen_high_flush_no_fold_sb_no_limp_and_monotone_j_fd() {
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

    let b_t7o = preflop_bucket(
        Card::new(Rank::Ten, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Clubs),
    )
    .0;
    let raw_limp = [0.10, 0.80, 0.10, 0.0, 0.0, 0.0];
    for _ in 0..20 {
        let (act_sb, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &raw_limp,
                legal_actions: &LEGAL_FULL,
                bucket: b_t7o,
                round: 1,
                to_call: 50,
                pot: 150,
                my_chips: 9950,
                ..Default::default()
            },
            &mut rng,
        );
        assert_ne!(
            act_sb, CALL_CHECK,
            "Tier 7 T7o on 100 BB HU SB must never limp"
        );
    }
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
    assert_ne!(
        act_turn, FOLD,
        "Pair + NFD (bucket 26) must not fold on 3-flush Turn (reasons: {:?})",
        trace_turn.guard_reasons
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
        let (act_river, tr) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &check_heavy,
                legal_actions: &legal_river,
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
fn test_monster_value_raise_river_halfpot_tp_and_maniac_adjust() {
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

    let mut maniac = OpponentTracker::new();
    for _ in 0..12 {
        maniac.record_action_street_detailed(RAISE_HALF_POT, 1, false, 300, 450);
        maniac.record_action_street_detailed(RAISE_HALF_POT, 2, false, 600, 1200);
        maniac.end_hand();
    }
    let bp = [0.20, 0.50, 0.15, 0.15, 0.0, 0.0];
    let adj = maniac.adjust_strategy(&ExploitRequest {
        blueprint_probs: &bp,
        legal_actions: &LEGAL_FULL,
        bucket: 45,
        round: 3,
        to_call: 400,
        table_bb: 100,
    });
    assert!(
        adj[CALL_CHECK as usize] > bp[CALL_CHECK as usize]
            && adj[FOLD as usize] < bp[FOLD as usize],
        "High-VPIP maniac must activate Maniac bluff-catching adjustment: {adj:?}"
    );
}

#[test]
fn test_flop_commit_draw_fold_and_multiway_cold_3bet_shove() {
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

    /* Multiway cold 3-bet shove (~19.4 BB over CO open) with KQs (bucket 15) must fold with zero prior reads */
    let (act_kqs_mw, _) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &[FOLD, CALL_CHECK],
            bucket: 15,
            round: 1,
            to_call: 15490,
            pot: 22160,
            active_opponents: 2,
            table_bb: 800,
            my_chips: 15490,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_kqs_mw, FOLD,
        "KQs must fold to a 19.4 BB multiway cold 3-bet squeeze shove with no reads"
    );
}
