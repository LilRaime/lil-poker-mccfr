use lil_poker_mccfr::cfr::opponent_model::{OpponentStyle, OpponentTracker};
use lil_poker_mccfr::game::holdem::{
    Card, Rank, Suit, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};

#[test]
fn test_opponent_tracker_advanced_metrics() {
    let mut tracker = OpponentTracker::new();

    /* First sequence: Preflop open raise, Flop c-bet faced and folded */
    tracker.record_action_street(RAISE_MIN, 1, false);
    tracker.record_action_street(FOLD, 2, true); /* fold to c-bet */
    tracker.end_hand();

    assert_eq!(tracker.total_hands, 1);
    assert_eq!(tracker.vpip_hands, 1);
    assert_eq!(tracker.pfr_hands, 1);
    assert_eq!(tracker.saw_flop, 1);
    assert_eq!(tracker.flop_cbet_faced, 1);
    assert_eq!(tracker.flop_fold_to_cbet, 1);

    /* Second sequence: Preflop limp/call, Flop call c-bet, Showdown reached */
    tracker.record_action_street(CALL_CHECK, 1, false);
    tracker.record_action_street(CALL_CHECK, 2, true); /* call c-bet */
    tracker.record_action_street(CALL_CHECK, 3, false);
    tracker.record_action_street(CALL_CHECK, 4, false);
    tracker.record_showdown();
    tracker.end_hand();

    assert_eq!(tracker.total_hands, 2);
    assert_eq!(tracker.vpip_hands, 2);
    assert_eq!(tracker.pfr_hands, 1); /* was limp, so pfr unchanged */
    assert_eq!(tracker.flop_cbet_faced, 2);
    assert_eq!(tracker.flop_fold_to_cbet, 1);
    assert_eq!(tracker.went_to_showdown, 1);
}

#[test]
fn test_opponent_style_classification_and_showdown() {
    let mut tracker = OpponentTracker::new();

    /* Simulate a Calling Station: VPIP 100%, almost zero folds, high calls */
    for _ in 0..10 {
        tracker.record_action_street(CALL_CHECK, 1, false);
        tracker.record_action_street(CALL_CHECK, 2, false);
        tracker.end_hand();
    }
    let (style, conf) = tracker.classify_style();
    assert_eq!(style, OpponentStyle::CallingStation);
    assert!(conf > 0.50);

    /* Record a showdown where opponent bluffed river with 7-2 air */
    let card = |rank, suit| Card { rank, suit };
    let opp_trash = [
        card(Rank::Seven, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];
    let board = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Spades),
        card(Rank::Nine, Suit::Diamonds),
        card(Rank::Three, Suit::Clubs),
    ];
    tracker.record_showdown_hand(opp_trash, &board, true);

    assert_eq!(tracker.showdown_hands, 1);
    assert_eq!(tracker.showdown_bluff_count, 1);
    assert_eq!(tracker.showdown_bluff_rate(), 1.0);
}

#[test]
fn test_opponent_barrel_metrics_and_wilson() {
    let mut tracker = OpponentTracker::new();

    /* Wilson score interval bounds checking */
    let (low, high) = OpponentTracker::wilson_confidence(50, 100, 1.96);
    assert!(
        (0.39..=0.42).contains(&low),
        "Wilson 95% lower bound for 50/100 should be ~0.40, got {}",
        low
    );
    assert!(
        (0.58..=0.61).contains(&high),
        "Wilson 95% upper bound for 50/100 should be ~0.60, got {}",
        high
    );

    /* Turn barrel facing and folding */
    tracker.record_action_street(RAISE_MIN, 1, false);
    tracker.record_action_street(CALL_CHECK, 2, false);
    tracker.record_action_street(FOLD, 3, true); /* facing turn cbet and folded! */
    tracker.end_hand();

    assert_eq!(tracker.turn_cbet_faced, 1);
    assert_eq!(tracker.turn_fold_to_cbet, 1);
    assert_eq!(tracker.turn_fold_to_cbet_ratio(), 1.0);

    /* River bet facing and folding */
    tracker.record_action_street(CALL_CHECK, 1, false);
    tracker.record_action_street(CALL_CHECK, 2, false);
    tracker.record_action_street(CALL_CHECK, 3, false);
    tracker.record_action_street(FOLD, 4, true); /* facing river bet and folded! */
    tracker.end_hand();

    /* Add realistic preflop folds so opponent represents an over-folding player */
    for _ in 0..4 {
        tracker.record_action_street(FOLD, 1, false);
        tracker.end_hand();
    }

    assert_eq!(tracker.river_bet_faced, 1);
    assert_eq!(tracker.river_fold_to_bet, 1);
    assert_eq!(tracker.river_fold_to_bet_ratio(), 1.0);

    /* Test turn barrel exploit in adjust_strategy_full */
    let base_strat = [0.30, 0.40, 0.10, 0.10, 0.10, 0.0];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT, RAISE_HALF_POT];
    // Weak hand (bucket 10) on Turn (round 3)
    let adj_turn = tracker.adjust_strategy_full(&base_strat, &legal, 10, 3);
    let orig_raise: f64 = base_strat[3] + base_strat[4];
    let adj_raise: f64 = adj_turn[3] + adj_turn[4];
    assert!(
        adj_raise > orig_raise,
        "Facing turn over-folder, bot should fire 2nd barrel bluff more often (orig {}, adj {})",
        orig_raise,
        adj_raise
    );
}

#[test]
fn test_opponent_sizing_tells() {
    let mut tracker = OpponentTracker::new();

    /* Record detailed actions with bet size and pot */
    // Opponent bet 20 chips into 100 pot (20% pot -> small bet)
    tracker.record_action_street_detailed(RAISE_MIN, 2, false, 20, 100);
    // Opponent bet 50 chips into 100 pot (50% pot -> mid bet)
    tracker.record_action_street_detailed(RAISE_HALF_POT, 2, false, 50, 100);
    // Opponent bet 150 chips into 100 pot (150% pot -> large/overbet)
    tracker.record_action_street_detailed(ALL_IN, 3, false, 150, 100);

    assert_eq!(tracker.sizing_small_bets, 1);
    assert_eq!(tracker.sizing_mid_bets, 1);
    assert_eq!(tracker.sizing_large_bets, 1);

    /* Record showdown with large bet and monster cards */
    let card = |rank, suit| Card { rank, suit };
    let opp_quads = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Clubs)];
    let board = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::King, Suit::Spades),
        card(Rank::Queen, Suit::Spades),
        card(Rank::Two, Suit::Diamonds),
    ];
    tracker.record_showdown_hand_detailed(opp_quads, &board, true, 800, 1000); /* 80% pot bet with Quads! */

    assert_eq!(tracker.showdown_large_monster_count, 1);
}

#[test]
fn test_opponent_positional_and_3bet_tracking() {
    let mut tracker = OpponentTracker::new();

    /* SB: 10 hands, 8 VPIP, 6 PFR */
    for _ in 0..6 {
        tracker.record_preflop_positional(true, true, true);
    }
    for _ in 0..2 {
        tracker.record_preflop_positional(true, true, false);
    }
    for _ in 0..2 {
        tracker.record_preflop_positional(true, false, false);
    }

    assert_eq!(tracker.sb_total_hands, 10);
    assert_eq!(tracker.sb_vpip_hands, 8);
    assert_eq!(tracker.sb_pfr_hands, 6);
    assert!((tracker.sb_vpip_ratio() - 0.80).abs() < 1e-6);
    assert!((tracker.sb_pfr_ratio() - 0.60).abs() < 1e-6);

    /* BB: 10 hands, 5 VPIP */
    for _ in 0..5 {
        tracker.record_preflop_positional(false, true, false);
    }
    for _ in 0..5 {
        tracker.record_preflop_positional(false, false, false);
    }
    assert_eq!(tracker.bb_total_hands, 10);
    assert_eq!(tracker.bb_vpip_hands, 5);
    assert!((tracker.bb_vpip_ratio() - 0.50).abs() < 1e-6);

    /* 3-bet facing: faced 4, folded 3 */
    tracker.record_facing_3bet(true);
    tracker.record_facing_3bet(true);
    tracker.record_facing_3bet(true);
    tracker.record_facing_3bet(false);
    assert_eq!(tracker.faced_3bet_count, 4);
    assert_eq!(tracker.fold_to_3bet_count, 3);
    assert!((tracker.fold_to_3bet_ratio() - 0.75).abs() < 1e-6);
}

#[test]
fn test_opponent_showdown_hand_tier_tracking() {
    let mut tracker = OpponentTracker::new();
    let card = |rank, suit| Card { rank, suit };

    let board = [
        card(Rank::King, Suit::Hearts),
        card(Rank::Eight, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
        card(Rank::Four, Suit::Spades),
        card(Rank::Nine, Suit::Diamonds),
    ];

    /* 1. Opponent shows down AKo (offsuit monster, index 91).
     * Must NOT be counted as loose trash!
     * Must be counted as a slowplay/trap. */
    let opp_ako = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    tracker.record_showdown_hand_detailed(opp_ako, &board, false, 0, 100);

    assert_eq!(
        tracker.showdown_loose_count, 0,
        "AKo is a Tier 1 hand and must NEVER increment showdown_loose_count"
    );
    assert_eq!(
        tracker.showdown_trap_count, 1,
        "AKo is a Tier 1 premium hand and should increment showdown_trap_count"
    );

    /* 2. Opponent shows down 72o (Tier 5 pure trash).
     * Must be counted as loose trash! */
    let opp_72o = [
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    ];
    tracker.record_showdown_hand_detailed(opp_72o, &board, false, 0, 100);

    assert_eq!(
        tracker.showdown_loose_count, 1,
        "72o is Tier 5 trash and must increment showdown_loose_count"
    );
}

#[test]
fn test_showdown_trap_false_positive_filtering_and_exploit() {
    use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;

    let mut tracker = OpponentTracker::new();
    let aa_hole = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Ace, Suit::Spades),
    ];
    let board = [
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Five, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Spades),
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Four, Suit::Clubs),
    ];

    /* 1. AA in a 3-bet pot (2 preflop raises) must NOT count as a trap */
    tracker.record_action_street_detailed(RAISE_HALF_POT, 1, false, 300, 150);
    tracker.record_action_street_detailed(RAISE_HALF_POT, 1, false, 900, 1200);
    tracker.record_showdown_hand_detailed(aa_hole, &board, false, 0, 1800);
    tracker.end_hand();
    assert_eq!(
        tracker.showdown_trap_count, 0,
        "AA in a 3-bet pot must not falsely increment showdown_trap_count"
    );

    /* 2. AA with a postflop raise must NOT count as a slowplay trap */
    tracker.record_action_street_detailed(RAISE_THIRD_POT, 2, false, 400, 400);
    tracker.record_showdown_hand_detailed(aa_hole, &board, false, 0, 1200);
    tracker.end_hand();
    assert_eq!(
        tracker.showdown_trap_count, 0,
        "AA with postflop aggression must not falsely increment showdown_trap_count"
    );

    /* 3. Passive AA (0-1 preflop raises, 0 postflop raises, opp_raised_river == false, last_bet_amt == 0) DOES count */
    tracker.record_action_street_detailed(CALL_CHECK, 1, false, 100, 150);
    tracker.record_action_street_detailed(CALL_CHECK, 2, false, 0, 200);
    tracker.record_showdown_hand_detailed(aa_hole, &board, false, 0, 200);
    tracker.end_hand();
    assert_eq!(tracker.showdown_trap_count, 1);

    /* One more genuine passive trap brings showdown_trap_count == 2 (ratio = 2/4 = 0.50 >= 0.25) */
    tracker.record_action_street_detailed(CALL_CHECK, 1, false, 100, 150);
    tracker.record_showdown_hand_detailed(
        [
            Card::new(Rank::King, Suit::Hearts),
            Card::new(Rank::King, Suit::Clubs),
        ],
        &board,
        false,
        0,
        200,
    );
    tracker.end_hand();
    assert_eq!(tracker.showdown_trap_count, 2);

    /* On River checked to us with marginal/medium equity (bucket 32), exploit trims bluff/big bets toward check-back */
    let legal = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let base: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.40, 0.10, 0.10, 0.30, 0.10];
    let adj = tracker.adjust_strategy_context(&base, &legal, 32, 4, 0, 1000);
    assert!(
        adj[CALL_CHECK as usize] > base[CALL_CHECK as usize]
            && adj[RAISE_HALF_POT as usize] < base[RAISE_HALF_POT as usize],
        "Against genuine showdown trapper, River check-back must rise ({:.3} -> {:.3}) and big bet drop ({:.3} -> {:.3})",
        base[CALL_CHECK as usize],
        adj[CALL_CHECK as usize],
        base[RAISE_HALF_POT as usize],
        adj[RAISE_HALF_POT as usize]
    );
}
