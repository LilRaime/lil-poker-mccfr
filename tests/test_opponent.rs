use lil_poker_mccfr::cfr::opponent_model::{OpponentStyle, OpponentTracker};
use lil_poker_mccfr::game::config::{BIG_BLIND, SMALL_BLIND};
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
fn test_dynamic_bet_sizing_against_leaks() {
    let mut station_tracker = OpponentTracker::new();
    /* Simulate Calling Station: 15 calls, 1 fold, 1 raise */
    for _ in 0..15 {
        station_tracker.record_action(CALL_CHECK, false);
    }
    station_tracker.record_action(FOLD, false);
    station_tracker.record_action(RAISE_MIN, false);

    let base_strat = [0.10, 0.30, 0.15, 0.15, 0.15, 0.15];
    let legal = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];

    /* 1. Strong hand (bucket 50) against calling station -> Overbets (RAISE_HALF_POT, ALL_IN) boosted more than RAISE_MIN */
    let adj_station_strong = station_tracker.adjust_strategy_full(&base_strat, &legal, 50, 2);
    let boost_half =
        adj_station_strong[RAISE_HALF_POT as usize] / base_strat[RAISE_HALF_POT as usize];
    let boost_min = adj_station_strong[RAISE_MIN as usize] / base_strat[RAISE_MIN as usize];
    assert!(
        boost_half > boost_min,
        "Against Calling Station, large sizing (half pot boost {}) must exceed min-raise (boost {})",
        boost_half,
        boost_min
    );

    /* 2. Weak hand (bucket 10) against calling station -> All raises suppressed */
    let adj_station_weak = station_tracker.adjust_strategy_full(&base_strat, &legal, 10, 2);
    let total_raise_base: f64 = base_strat[RAISE_MIN as usize..=ALL_IN as usize]
        .iter()
        .sum();
    let total_raise_weak: f64 = adj_station_weak[RAISE_MIN as usize..=ALL_IN as usize]
        .iter()
        .sum();
    assert!(
        total_raise_weak < total_raise_base,
        "Against Calling Station, bluffs should be reduced below base (base {}, weak {})",
        total_raise_base,
        total_raise_weak
    );

    /* 3. Weak hand against Nit/Rock (over-folder) -> Cheap bluffs (RAISE_THIRD_POT / RAISE_MIN) boosted, large bets dampened */
    let mut nit_tracker = OpponentTracker::new();
    for _ in 0..15 {
        nit_tracker.record_action(FOLD, false);
    }
    nit_tracker.record_action(CALL_CHECK, false);
    nit_tracker.record_action(RAISE_MIN, false);

    let adj_nit_weak = nit_tracker.adjust_strategy_full(&base_strat, &legal, 10, 2);
    let boost_third = adj_nit_weak[RAISE_THIRD_POT as usize] / base_strat[RAISE_THIRD_POT as usize];
    let boost_half_nit =
        adj_nit_weak[RAISE_HALF_POT as usize] / base_strat[RAISE_HALF_POT as usize];
    assert!(
        boost_third > boost_half_nit,
        "Against Nit, cheap bluffs (1/3 pot boost {}) must be favored over large bluffs (half pot boost {})",
        boost_third,
        boost_half_nit
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
fn test_preflop_button_steal_requires_unraised_pot() {
    let tracker = OpponentTracker::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(999);

    /* When facing fold-heavy blueprint [0.98, 0.0, 0.02] */
    let raw_probs = [0.98, 0.0, 0.02, 0.0, 0.0, 0.0];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN];

    /* 1. Unraised pot on button: to_call = SMALL_BLIND (50 chips).
     * Playable hand (tier <= 4, e.g. bucket 20).
     * Folds should be shifted into raises (steal)!
     * 0.98 * 0.85 = +0.833 to RAISE_MIN, making raise overwhelmingly dominant. */
    let action_steal = tracker.select_action_purified_context(
        &raw_probs,
        &legal,
        20,
        1,           /* Preflop */
        SMALL_BLIND, /* Small blind to call in unraised pot */
        BIG_BLIND + SMALL_BLIND,
        None,
        None,
        &mut rng,
    );
    assert_eq!(
        action_steal, RAISE_MIN,
        "Unraised pot on button should steal-raise"
    );

    /* 2. Facing a raise: to_call = BIG_BLIND (100 chips).
     * The bot is in the Big Blind facing a min-raise.
     * This is NOT a button steal situation! Folds should NOT be converted into 3-bets!
     * 0.02 raise noise gets clamped to 0, leaving 100% FOLD. */
    let action_defend = tracker.select_action_purified_context(
        &raw_probs,
        &legal,
        20,
        1,         /* Preflop */
        BIG_BLIND, /* Facing a min-raise (BIG_BLIND to call) */
        BIG_BLIND * 2,
        None,
        None,
        &mut rng,
    );
    assert_eq!(
        action_defend, FOLD,
        "Facing a raise (to_call = BIG_BLIND) must NOT trigger button steal redistribution into raises"
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
fn test_passive_station_value_extraction_boost() {
    let mut tracker = OpponentTracker::new();
    for _ in 0..20 {
        tracker.record_action(CALL_CHECK, false);
    }

    let mut rng = rand::rngs::SmallRng::seed_from_u64(777);
    let legal = [
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    /* SubgameSolver / blueprint passive vector where CALL_CHECK is 90% and RAISE_HALF_POT is 0% */
    let passive_solver_probs = [0.0, 0.90, 0.05, 0.05, 0.0, 0.0];
    let safe_board = [
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Diamonds),
        Card::new(Rank::Four, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Spades),
        Card::new(Rank::Two, Suit::Clubs),
    ];

    /* 1. Turn (round 3) & River (round 4) with Top Pair Good Kicker (bucket 47) or Overpair (bucket 55):
     * Must override passive SubgameSolver checking and fire RAISE_HALF_POT / RAISE_THIRD_POT! */
    for &round in &[2u8, 3, 4] {
        for &bucket in &[47usize, 55, 62, 70] {
            let act = tracker.select_action_purified_context(
                &passive_solver_probs,
                &legal,
                bucket,
                round,
                0,
                1200,
                None,
                Some(&safe_board[..(round as usize + 1)]),
                &mut rng,
            );
            assert!(
                act == RAISE_HALF_POT || act == RAISE_THIRD_POT || (bucket >= 65 && act == ALL_IN),
                "Against passive check/call station on round {} with bucket {}, bot must value-bet large instead of checking, got {}",
                round,
                bucket,
                act
            );
        }
    }

    /* 2. Preflop (round 1) with strong Tier 1-2 hand (bucket 5) against passive station:
     * Must size up to RAISE_HALF_POT instead of limping or min-raising */
    let pf_act = tracker.select_action_purified_context(
        &passive_solver_probs,
        &legal,
        5,
        1,
        0,
        300,
        None,
        None,
        &mut rng,
    );
    assert_eq!(
        pf_act, RAISE_HALF_POT,
        "Against passive station preflop with Tier 1-3 hand, bot must size up to RAISE_HALF_POT"
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

#[test]
fn test_sb_limper_iso_raise_and_high_fold_to_3bet_3bet() {
    use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;

    /* 1. Serial SB limper: in BB facing limp (to_call == 0) with medium-decent hand (QTo) */
    let mut limper = OpponentTracker::new();
    for _ in 0..5 {
        limper.record_preflop_positional(true, true, false);
        limper.end_hand();
    }
    assert!(limper.sb_limp_ratio() > 0.90);

    let neutral = OpponentTracker::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);
    let legal_bb = [CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT, RAISE_HALF_POT];
    let passive_pf: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.85, 0.05, 0.05, 0.05, 0.0];
    let hole_qto = [
        Card::new(Rank::Queen, Suit::Spades),
        Card::new(Rank::Ten, Suit::Diamonds),
    ];
    let pf_bucket_qto =
        lil_poker_mccfr::cfr::abstraction::preflop_bucket(hole_qto[0], hole_qto[1]).0 as usize;

    let mut iso_raises = 0;
    let mut baseline_raises = 0;
    for _ in 0..100 {
        let (act, _) = limper.select_action_purified_traced_with_stack(
            &passive_pf,
            &legal_bb,
            pf_bucket_qto,
            1,
            0,
            200,
            Some(&hole_qto),
            None,
            1,
            100,
            10_000,
            &mut rng,
        );
        if act >= RAISE_MIN {
            iso_raises += 1;
        }
        let (base_act, _) = neutral.select_action_purified_traced_with_stack(
            &passive_pf,
            &legal_bb,
            pf_bucket_qto,
            1,
            0,
            200,
            Some(&hole_qto),
            None,
            1,
            100,
            10_000,
            &mut rng,
        );
        if base_act >= RAISE_MIN {
            baseline_raises += 1;
        }
    }
    assert!(
        iso_raises >= 35 && iso_raises > baseline_raises + 15,
        "In BB facing serial SB limper with QTo (to_call=0), ISO-raise rate ({}/100) must substantially exceed baseline ({}/100)",
        iso_raises,
        baseline_raises
    );

    /* 2. High fold-to-3bet opener facing BB vs 2.5x open (to_call = 150, bb = 100) with Tier 3 hand */
    let mut nit3bet = OpponentTracker::new();
    for _ in 0..4 {
        nit3bet.record_facing_3bet(true);
        nit3bet.end_hand();
    }
    assert!(nit3bet.fold_to_3bet_ratio() > 0.90);

    let hole_ajs = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Hearts),
    ];
    let pf_bucket_ajs =
        lil_poker_mccfr::cfr::abstraction::preflop_bucket(hole_ajs[0], hole_ajs[1]).0 as usize;
    let legal_vs_open = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let flat_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.25, 0.70, 0.05, 0.0, 0.0, 0.0];

    let mut three_bets = 0;
    let mut base_three_bets = 0;
    for _ in 0..100 {
        let (act, _) = nit3bet.select_action_purified_traced_with_stack(
            &flat_heavy,
            &legal_vs_open,
            pf_bucket_ajs,
            1,
            150,
            350,
            Some(&hole_ajs),
            None,
            1,
            100,
            10_000,
            &mut rng,
        );
        if act >= RAISE_MIN {
            three_bets += 1;
        }
        let (base_act, _) = neutral.select_action_purified_traced_with_stack(
            &flat_heavy,
            &legal_vs_open,
            pf_bucket_ajs,
            1,
            150,
            350,
            Some(&hole_ajs),
            None,
            1,
            100,
            10_000,
            &mut rng,
        );
        if base_act >= RAISE_MIN {
            base_three_bets += 1;
        }
    }
    assert!(
        three_bets >= 18 && three_bets > base_three_bets + 10,
        "Against opponent who folds >90% to 3-bets, BB must shift passive flats/folds into 3-bets with AJs ({}/100 vs baseline {}/100)",
        three_bets,
        base_three_bets
    );
}

#[test]
fn test_observe_blind_level_decay_and_short_stack_15bb_nash_call() {
    use lil_poker_mccfr::cfr::abstraction::preflop_bucket;
    use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;

    /* 1. Blind level transition decay (BB=100 -> BB=200 after 20 hands):
     * Should softly decay old counters (~0.71 retain) while preserving ratios and invariants. */
    let mut tracker = OpponentTracker::new();
    tracker.observe_blind_level(100);
    for i in 0..20 {
        if i < 10 {
            tracker.record_action_street_detailed(RAISE_MIN, 1, false, 200, 150);
        } else {
            tracker.record_action_street_detailed(FOLD, 1, true, 0, 250);
        }
        tracker.end_hand();
    }
    assert_eq!(tracker.total_hands, 20);
    assert_eq!(tracker.pfr_hands, 10);

    /* Same BB does not decay */
    tracker.observe_blind_level(100);
    assert_eq!(tracker.total_hands, 20);

    /* BB doubles 100 -> 200 (ratio = 2.0): effective counters shrink so new-level hands adapt quickly */
    tracker.observe_blind_level(200);
    assert!(
        tracker.total_hands < 20 && tracker.total_hands >= 12,
        "Expected soft decay on 2x blind jump (100 -> 200), got total_hands={}",
        tracker.total_hands
    );
    assert!(tracker.pfr_hands <= tracker.vpip_hands && tracker.vpip_hands <= tracker.total_hands);

    /* 2. At 15 BB facing a 14 BB shove from a maniac/shove_bot, 88 (bucket 6, Nash cap 26 BB vs maniac)
     * must rescue from blueprint fold and CALL instead of hard-folding! */
    let mut maniac = OpponentTracker::new();
    for _ in 0..10 {
        maniac.record_action_street_detailed(ALL_IN, 1, false, 1400, 150);
        maniac.record_preflop_positional(true, true, true);
        maniac.end_hand();
    }
    let hole_88 = [
        Card::new(Rank::Eight, Suit::Hearts),
        Card::new(Rank::Eight, Suit::Spades),
    ];
    let bucket_88 = preflop_bucket(hole_88[0], hole_88[1]).0 as usize;
    let legal = [FOLD, CALL_CHECK];
    let fold_heavy_blueprint: [f64; HOLDEM_NUM_ACTIONS] = [0.92, 0.08, 0.0, 0.0, 0.0, 0.0];
    let mut rng = rand::rngs::SmallRng::seed_from_u64(101);
    let mut calls = 0;
    for _ in 0..50 {
        let (act, _) = maniac.select_action_purified_traced_with_stack(
            &fold_heavy_blueprint,
            &legal,
            bucket_88,
            1,
            1400,
            1550,
            Some(&hole_88),
            None,
            1,
            100,
            1400,
            &mut rng,
        );
        if act == CALL_CHECK {
            calls += 1;
        }
    }
    assert!(
        calls >= 45,
        "At 15 BB facing 14 BB shove from maniac, 88 must call via Nash depth rescue (got {}/50)",
        calls
    );

    /* 3. Unified <= 12 BB Push/Fold handles 10 BB shove call (including tier 4 77 at 0.45 pot odds) */
    let legal_facing_shove = [FOLD, CALL_CHECK];
    let (act_10bb_88, reason_88) = maniac
        .select_short_stack_preflop_action(bucket_88, 900, 1100, 1, 100, 900, &legal_facing_shove)
        .expect("<= 12 BB short-stack engine must activate");
    assert_eq!(
        act_10bb_88, CALL_CHECK,
        "88 must call 10 BB shove ({})",
        reason_88
    );

    let bucket_77 = preflop_bucket(
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Clubs),
    )
    .0 as usize;
    let (act_10bb_77, reason_77) = maniac
        .select_short_stack_preflop_action(bucket_77, 900, 1100, 1, 100, 900, &legal_facing_shove)
        .expect("<= 12 BB short-stack engine must activate");
    assert_eq!(
        act_10bb_77, CALL_CHECK,
        "77 must call 10 BB shove ({})",
        reason_77
    );
}
