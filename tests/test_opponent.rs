use lil_poker_mccfr::cfr::opponent_model::{OpponentStyle, OpponentTracker};
use lil_poker_mccfr::game::holdem::{
    Card, Rank, Suit, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use rand::SeedableRng;

#[test]
fn test_opponent_model_context_adjustment() {
    let mut tracker = OpponentTracker::new();
    /* Simulate calling station: 15 calls, 1 fold, 1 raise */
    for _ in 0..15 {
        tracker.record_action(CALL_CHECK, false);
    }
    tracker.record_action(FOLD, false);
    tracker.record_action(RAISE_MIN, false);

    let base_strat = [0.15, 0.35, 0.15, 0.15, 0.10, 0.10];
    let legal = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];

    /* Strong hand (bucket 55) against calling station -> raise probability must increase */
    let adj_strong = tracker.adjust_strategy_with_context(&base_strat, &legal, 55, false);
    let total_raise_base: f64 = base_strat[RAISE_MIN as usize..=ALL_IN as usize]
        .iter()
        .sum();
    let total_raise_adj_strong: f64 = adj_strong[RAISE_MIN as usize..=ALL_IN as usize]
        .iter()
        .sum();
    assert!(
        total_raise_adj_strong > total_raise_base,
        "Against calling station, strong hand should raise more"
    );

    /* Weak hand (bucket 10) against calling station -> bluff raises must decrease */
    let adj_weak = tracker.adjust_strategy_with_context(&base_strat, &legal, 10, false);
    let total_raise_adj_weak: f64 = adj_weak[RAISE_MIN as usize..=ALL_IN as usize].iter().sum();
    assert!(
        total_raise_adj_weak < total_raise_base,
        "Against calling station, weak hand bluffs should be reduced"
    );
}

#[test]
fn test_select_action_purified_defense() {
    let tracker = OpponentTracker::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);

    /* 1. Preflop trash hand (bucket 130) facing All-in (to_call = 500)
     * Even if raw_probs has some call probability (e.g. 0.40 call, 0.60 fold),
     * purified selection MUST force FOLD! */
    let raw_probs = [0.60, 0.40, 0.0, 0.0, 0.0, 0.0];
    let legal = [FOLD, CALL_CHECK];
    let chosen = tracker.select_action_purified(&raw_probs, &legal, 130, true, 500, &mut rng);
    assert_eq!(chosen, FOLD, "Preflop trash facing shove must fold!");

    /* 2. Preflop monster hand (bucket 5) facing All-in (to_call = 500)
     * Monster hands should call/raise! */
    let raw_probs_monster = [0.05, 0.85, 0.10, 0.0, 0.0, 0.0];
    let chosen_monster =
        tracker.select_action_purified(&raw_probs_monster, &legal, 5, true, 500, &mut rng);
    assert_eq!(
        chosen_monster, CALL_CHECK,
        "Preflop monster facing shove must not fold!"
    );

    /* 3. Noise clamping: tiny 2% probability should be clamped to 0 */
    let raw_probs_noise = [0.02, 0.98];
    let chosen_clean =
        tracker.select_action_purified(&raw_probs_noise, &legal, 50, false, 0, &mut rng);
    assert_eq!(chosen_clean, CALL_CHECK, "Noise should be clamped");
}

#[test]
fn test_opponent_tracker_advanced_metrics() {
    let mut tracker = OpponentTracker::new();

    /* Hand 1: Preflop open raise, Flop c-bet faced and folded */
    tracker.record_action_street(RAISE_MIN, 1, false);
    tracker.record_action_street(FOLD, 2, true); /* fold to c-bet */
    tracker.end_hand();

    assert_eq!(tracker.total_hands, 1);
    assert_eq!(tracker.vpip_hands, 1);
    assert_eq!(tracker.pfr_hands, 1);
    assert_eq!(tracker.saw_flop, 1);
    assert_eq!(tracker.flop_cbet_faced, 1);
    assert_eq!(tracker.flop_fold_to_cbet, 1);

    /* Hand 2: Preflop limp/call, Flop call c-bet, Showdown reached */
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
fn test_exploit_guardrails_and_toggles() {
    let mut tracker = OpponentTracker::new();

    /* Fill tracker with extreme calling station stats (30 calls, 0 folds) */
    for _ in 0..30 {
        tracker.record_action(CALL_CHECK, false);
    }
    assert_eq!(tracker.fold_count, 0);
    assert_eq!(tracker.call_count, 30);

    let bp = vec![0.30, 0.40, 0.15, 0.10, 0.05, 0.0];
    let legal = vec![FOLD, CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT, RAISE_HALF_POT];

    /* 1. When exploit is disabled: MUST return identical blueprint probabilities! */
    tracker.set_exploit_enabled(false);
    let disabled_adj = tracker.adjust_strategy_full(&bp, &legal, 10, 2);
    for i in 0..5 {
        assert!(
            (disabled_adj[i] - bp[i]).abs() < 1e-6,
            "When disabled, probabilities must exactly match blueprint"
        );
    }

    /* 2. When exploit is enabled: adjusted probabilities must be clamped within safe guardrails [0.15*bp, 2.50*bp] */
    tracker.set_exploit_enabled(true);
    tracker.set_exploit_factor(0.70);
    let enabled_adj = tracker.adjust_strategy_full(&bp, &legal, 10, 2);
    for &a in &legal {
        let idx = a as usize;
        /* Even for extreme calling station where bluffing is suppressed, probability cannot drop below 0.15 * bp */
        assert!(
            enabled_adj[idx] >= 0.10 * bp[idx],
            "Probability should never drop below safe floor. Got {} vs bp {}",
            enabled_adj[idx],
            bp[idx]
        );
        /* And probability cannot explode above 2.50 * bp */
        assert!(
            enabled_adj[idx] <= 2.60 * bp[idx] + 0.05,
            "Probability should never explode above safe ceiling. Got {} vs bp {}",
            enabled_adj[idx],
            bp[idx]
        );
    }
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
fn test_limper_exploitation_and_float_probe() {
    let mut tracker = OpponentTracker::new();

    /* SB limper: 10 hands on SB, 8 VPIP, 1 PFR -> 7 limps (limp_ratio = 70%) */
    for _ in 0..7 {
        tracker.record_preflop_positional(true, true, false);
    }
    tracker.record_preflop_positional(true, true, true);
    for _ in 0..2 {
        tracker.record_preflop_positional(true, false, false);
    }
    assert!((tracker.sb_limp_ratio() - 0.70).abs() < 1e-6);

    /* Flop passive checker: 8 flop actions, 7 checks, 1 bet -> check_ratio = 87.5% */
    for _ in 0..7 {
        tracker.record_action_street(CALL_CHECK, 2, false);
    }
    tracker.record_action_street(RAISE_MIN, 2, false);
    assert!(tracker.flop_check_ratio() > 0.80);

    /* Test 1: Preflop ISO-raise against limper on BB (bucket 40, to_call = 0) */
    let base_strat = [0.0, 0.60, 0.20, 0.20, 0.0, 0.0];
    let legal = [CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT];
    let adj_limp = tracker.adjust_strategy_full(&base_strat, &legal, 40, 1);
    let total_raise_limp = adj_limp[RAISE_MIN as usize] + adj_limp[RAISE_THIRD_POT as usize];
    let total_raise_base = base_strat[RAISE_MIN as usize] + base_strat[RAISE_THIRD_POT as usize];
    assert!(
        total_raise_limp > total_raise_base,
        "ISO-raise against frequent limper must increase. Got {} vs base {}",
        total_raise_limp,
        total_raise_base
    );

    /* Test 2: Turn Float/Probe on missed flop c-bet (round 3) */
    let adj_turn = tracker.adjust_strategy_full(&base_strat, &legal, 30, 3);
    let total_raise_turn = adj_turn[RAISE_MIN as usize] + adj_turn[RAISE_THIRD_POT as usize];
    assert!(
        total_raise_turn > total_raise_base,
        "Turn probe bet against missed c-bet must increase"
    );
}

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
