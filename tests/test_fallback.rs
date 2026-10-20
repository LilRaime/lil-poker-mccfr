use lil_poker_mccfr::cfr::fallback::get_holdem_fallback_strategy;
use lil_poker_mccfr::game::holdem::{
    Card, Rank, Suit, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};

#[test]
fn test_fallback_strategy_preflop_and_postflop() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. Pocket Aces preflop: should NEVER fold, high raise probability */
    let hole_aa = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts)];
    let board_empty = [];
    let legal_facing_bet = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let strat_aa =
        get_holdem_fallback_strategy(&hole_aa, &board_empty, 1, 20, 40, &legal_facing_bet);
    assert_eq!(
        strat_aa[FOLD as usize], 0.0,
        "Pocket Aces should never fold"
    );
    assert!(
        strat_aa[RAISE_HALF_POT as usize]
            + strat_aa[RAISE_THIRD_POT as usize]
            + strat_aa[RAISE_MIN as usize]
            + strat_aa[ALL_IN as usize]
            > 0.50,
        "Pocket Aces should raise frequently"
    );

    /* 2. 7-2 offsuit preflop facing bet: should fold > 90% */
    let hole_72 = [
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    ];
    let strat_72 =
        get_holdem_fallback_strategy(&hole_72, &board_empty, 1, 20, 40, &legal_facing_bet);
    assert!(
        strat_72[FOLD as usize] > 0.90,
        "72o facing bet should fold > 90%, got {}",
        strat_72[FOLD as usize]
    );

    /* 3. Flush on River: should NEVER fold, even facing bet */
    let hole_fl = [card(Rank::Ace, Suit::Spades), card(Rank::Ten, Suit::Spades)];
    let board_fl = [
        card(Rank::Eight, Suit::Spades),
        card(Rank::Six, Suit::Spades),
        card(Rank::Two, Suit::Spades),
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
    ];
    let strat_fl =
        get_holdem_fallback_strategy(&hole_fl, &board_fl, 4, 100, 300, &legal_facing_bet);
    assert_eq!(
        strat_fl[FOLD as usize], 0.0,
        "Nut flush on river should never fold"
    );
    assert!(
        strat_fl[RAISE_HALF_POT as usize]
            + strat_fl[RAISE_THIRD_POT as usize]
            + strat_fl[RAISE_MIN as usize]
            + strat_fl[ALL_IN as usize]
            > 0.50,
        "Nut flush should raise for value"
    );

    /* 4. Complete Air on Flop when check is free: should check > 85%, never fold */
    let board_air = [
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Nine, Suit::Spades),
    ];
    let legal_free = [
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let strat_air = get_holdem_fallback_strategy(&hole_72, &board_air, 2, 0, 100, &legal_free);
    assert_eq!(strat_air[FOLD as usize], 0.0);
    assert!(
        strat_air[CALL_CHECK as usize] >= 0.85,
        "Checking should be dominant when check is free with air"
    );
}

#[test]
fn test_strategy_lookup_index() {
    use lil_poker_mccfr::cfr::fallback::StrategyLookupIndex;
    use std::collections::HashMap;

    let card = |rank, suit| Card { rank, suit };
    let hole_aa = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts)];

    let mut map = HashMap::new();
    map.insert("P:AA/c".to_string(), vec![0.0, 0.2, 0.4, 0.2, 0.2, 0.0]);
    map.insert("P:AA/r".to_string(), vec![0.0, 0.0, 0.2, 0.4, 0.3, 0.1]);
    map.insert("F:B10/c".to_string(), vec![0.1, 0.5, 0.2, 0.1, 0.1, 0.0]);
    map.insert(
        "P:c|F:B10/r".to_string(),
        vec![0.1, 0.3, 0.2, 0.3, 0.1, 0.0],
    );

    let index = StrategyLookupIndex::build_from_map(&map);

    // 1. Preflop lookup
    let pf_avg = index
        .query_bucket_avg(1, &hole_aa, &[], 0)
        .expect("preflop bucket avg");
    // Average of [0.0, 0.2, 0.4, 0.2, 0.2, 0.0] and [0.0, 0.0, 0.2, 0.4, 0.3, 0.1]
    assert!((pf_avg[0] - 0.0).abs() < 1e-6);
    assert!((pf_avg[1] - 0.1).abs() < 1e-6);
    assert!((pf_avg[2] - 0.3).abs() < 1e-6);
    assert!((pf_avg[3] - 0.3).abs() < 1e-6);

    // 2. Blended fallback
    let legal = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let blended = index.get_blended_fallback(&hole_aa, &[], 1, 20, 50, &legal);
    assert_eq!(
        blended.len(),
        lil_poker_mccfr::game::holdem::HOLDEM_NUM_ACTIONS
    );
    let sum: f64 = blended.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-6,
        "Sum of probabilities must equal 1.0"
    );
    assert_eq!(blended[FOLD as usize], 0.0, "Pocket Aces should not fold");
}

#[test]
fn test_binary_strategy_save_and_load() {
    use lil_poker_mccfr::cfr::holdem_mccfr::HoldemMCCFRSolver;
    let solver = HoldemMCCFRSolver::new();
    let temp_path = "/tmp/test_binary_strategy.bin";

    solver.train(100, 1, 0);
    let count = solver
        .save_strategy_to_file(temp_path)
        .expect("Save binary failed");
    assert!(count > 0);

    let loaded = HoldemMCCFRSolver::load_strategy_from_file(temp_path).expect("Load binary failed");
    assert_eq!(loaded.len(), count);

    let loaded_via_sim =
        lil_poker_mccfr::sim::holdem::load_holdem_strategy(temp_path).expect("Load via sim failed");
    assert_eq!(loaded_via_sim.len(), count);

    let _ = std::fs::remove_file(temp_path);
}

#[test]
fn test_preflop_strength_tiers_and_88_shove_defense() {
    use lil_poker_mccfr::cfr::abstraction::{preflop_bucket, preflop_strength_tier};

    let card = |rank, suit| Card { rank, suit };

    /* Tier 1: AA, KK, QQ, AKs, AKo */
    let (aa_idx, _) = preflop_bucket(card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts));
    let (aks_idx, _) = preflop_bucket(
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Spades),
    );
    let (ako_idx, _) = preflop_bucket(
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    );
    assert_eq!(preflop_strength_tier(aa_idx), 1);
    assert_eq!(preflop_strength_tier(aks_idx), 1);
    assert_eq!(preflop_strength_tier(ako_idx), 1);

    /* Tier 2: JJ, TT, 99, AQs, AQo, KQo */
    let (jj_idx, _) = preflop_bucket(
        card(Rank::Jack, Suit::Spades),
        card(Rank::Jack, Suit::Hearts),
    );
    let (aqs_idx, _) = preflop_bucket(
        card(Rank::Ace, Suit::Spades),
        card(Rank::Queen, Suit::Spades),
    );
    let (aqo_idx, _) = preflop_bucket(
        card(Rank::Ace, Suit::Spades),
        card(Rank::Queen, Suit::Hearts),
    );
    assert_eq!(preflop_strength_tier(jj_idx), 2);
    assert_eq!(preflop_strength_tier(aqs_idx), 2);
    assert_eq!(preflop_strength_tier(aqo_idx), 2);

    /* Tier 3: 88, 77, 66, A9s, KJs, JTs, 76s, AJo */
    let (eight_idx, _) = preflop_bucket(
        card(Rank::Eight, Suit::Spades),
        card(Rank::Eight, Suit::Hearts),
    );
    let (ajo_idx, _) = preflop_bucket(
        card(Rank::Ace, Suit::Spades),
        card(Rank::Jack, Suit::Hearts),
    );
    assert_eq!(preflop_strength_tier(eight_idx), 3);
    assert_eq!(preflop_strength_tier(ajo_idx), 3);

    /* Tier 4: 22, 55, K5s */
    let (two_idx, _) = preflop_bucket(card(Rank::Two, Suit::Spades), card(Rank::Two, Suit::Hearts));
    assert_eq!(preflop_strength_tier(two_idx), 4);

    /* Tier 5: 72o */
    let (trash_idx, _) = preflop_bucket(
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    );
    assert_eq!(preflop_strength_tier(trash_idx), 5);

    /* Check fallback strategy for 88 facing a large 35+ BB shove */
    let hole_88 = [
        card(Rank::Eight, Suit::Spades),
        card(Rank::Eight, Suit::Hearts),
    ];
    let legal = [FOLD, CALL_CHECK, ALL_IN];
    let strat_88_shove = get_holdem_fallback_strategy(&hole_88, &[], 1, 4000, 8000, &legal);
    assert!(
        strat_88_shove[FOLD as usize] >= 0.70,
        "Pocket 88 facing 40 BB shove in fallback must fold >= 70%, got {}",
        strat_88_shove[FOLD as usize]
    );
}
