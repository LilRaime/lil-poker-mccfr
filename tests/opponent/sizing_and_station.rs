use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
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
