use lil_poker_mccfr::cfr::fallback::{get_holdem_fallback_strategy, FallbackRequest};
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
    let strat_aa = get_holdem_fallback_strategy(&FallbackRequest {
        hole: hole_aa,
        board: &board_empty,
        round: 1,
        to_call: 20,
        pot: 40,
        legal: &legal_facing_bet,
        ..Default::default()
    });
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
    let strat_72 = get_holdem_fallback_strategy(&FallbackRequest {
        hole: hole_72,
        board: &board_empty,
        round: 1,
        to_call: 20,
        pot: 40,
        legal: &legal_facing_bet,
        ..Default::default()
    });
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
    let strat_fl = get_holdem_fallback_strategy(&FallbackRequest {
        hole: hole_fl,
        board: &board_fl,
        round: 4,
        to_call: 100,
        pot: 300,
        legal: &legal_facing_bet,
        ..Default::default()
    });
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
    let strat_air = get_holdem_fallback_strategy(&FallbackRequest {
        hole: hole_72,
        board: &board_air,
        round: 2,
        to_call: 0,
        pot: 100,
        legal: &legal_free,
        ..Default::default()
    });
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

    /* 1. Preflop lookup */
    let pf_avg = index
        .query_bucket_avg(1, &hole_aa, &[], 0)
        .expect("preflop bucket avg");
    /* Average of [0.0, 0.2, 0.4, 0.2, 0.2, 0.0] and [0.0, 0.0, 0.2, 0.4, 0.3, 0.1] */
    assert!((pf_avg[0] - 0.0).abs() < 1e-6);
    assert!((pf_avg[1] - 0.1).abs() < 1e-6);
    assert!((pf_avg[2] - 0.3).abs() < 1e-6);
    assert!((pf_avg[3] - 0.3).abs() < 1e-6);

    /* 2. Blended fallback */
    let legal = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let blended = index.get_blended_fallback(&FallbackRequest {
        hole: hole_aa,
        round: 1,
        to_call: 20,
        pot: 50,
        legal: &legal,
        ..Default::default()
    });
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

    /* Tier 5: 22, 44, K5s (Speculative / Late-Position Open in 8-tier scale) */
    let (two_idx, _) = preflop_bucket(card(Rank::Two, Suit::Spades), card(Rank::Two, Suit::Hearts));
    assert_eq!(preflop_strength_tier(two_idx), 5);

    /* Tier 8: 72o (Pure Trash in 8-tier scale) */
    let (trash_idx, _) = preflop_bucket(
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    );
    assert_eq!(preflop_strength_tier(trash_idx), 8);

    /* Check fallback strategy for 88 facing a large 35+ BB shove */
    let hole_88 = [
        card(Rank::Eight, Suit::Spades),
        card(Rank::Eight, Suit::Hearts),
    ];
    let legal = [FOLD, CALL_CHECK, ALL_IN];
    let strat_88_shove = get_holdem_fallback_strategy(&FallbackRequest {
        hole: hole_88,
        round: 1,
        to_call: 4000,
        pot: 8000,
        legal: &legal,
        ..Default::default()
    });
    assert!(
        strat_88_shove[FOLD as usize] >= 0.70,
        "Pocket 88 facing 40 BB shove in fallback must fold >= 70%, got {}",
        strat_88_shove[FOLD as usize]
    );
}

#[test]
fn test_fallback_high_blinds_and_turn_owned_lookup() {
    use lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket;
    use lil_poker_mccfr::cfr::fallback::lookup_strategy_hierarchical_owned;
    use lil_poker_mccfr::game::holdem::{RoundHistory, NUM_ROUNDS};
    use std::collections::HashMap;

    /* 1. Fallback BB scale: calling 400 into 1000 with Tier 5 (44) at table_bb=400 (1 BB) vs table_bb=100 (4 BB) */
    let hole_44 = [
        Card::new(Rank::Four, Suit::Spades),
        Card::new(Rank::Four, Suit::Hearts),
    ];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN];
    let strat_bb400 = get_holdem_fallback_strategy(&FallbackRequest {
        hole: hole_44,
        round: 1,
        to_call: 400,
        pot: 1000,
        legal: &legal,
        table_bb: 400,
        ..Default::default()
    });
    let strat_bb100 = get_holdem_fallback_strategy(&FallbackRequest {
        hole: hole_44,
        round: 1,
        to_call: 400,
        pot: 1000,
        legal: &legal,
        table_bb: 100,
        ..Default::default()
    });
    assert!(
        strat_bb400[CALL_CHECK as usize] > strat_bb100[CALL_CHECK as usize] + 0.2,
        "At table_bb=400, 400 chips is 1 BB (call ~0.40), whereas at bb=100 it is 4 BB (fold ~0.96)"
    );

    /* 2. Turn owned hierarchical lookup recovers P:rc|F:cc|T:... when exact P:rc|F:cr|T:... is missing */
    let hole_ak = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
    ];
    let board_turn = [
        Card::new(Rank::Ace, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Clubs),
        Card::new(Rank::Four, Suit::Spades),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let mut history = [RoundHistory::new(); NUM_ROUNDS];
    history[0].push(RAISE_MIN);
    history[0].push(CALL_CHECK);
    history[1].push(CALL_CHECK);
    history[1].push(RAISE_MIN);

    let bucket = postflop_equity_bucket(&hole_ak, &board_turn);
    let canonical_turn_key = format!("P:rc|F:cc|T:B{bucket}/");
    let mut map: HashMap<String, Vec<f64>> = HashMap::new();
    map.insert(canonical_turn_key, vec![0.0, 0.2, 0.3, 0.3, 0.2, 0.0]);

    let found = lookup_strategy_hierarchical_owned(
        |k| map.get(k).cloned(),
        &hole_ak,
        &board_turn,
        3,
        &history,
    );
    assert!(
        found.is_some(),
        "lookup_strategy_hierarchical_owned must fall back to P:rc|F:cc|T:... on Turn"
    );
}

#[test]
fn test_tier5_truncation_preserves_facing_bet_parity() {
    use lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket;
    use lil_poker_mccfr::cfr::fallback::lookup_strategy_hierarchical_owned;
    use lil_poker_mccfr::game::config::NUM_ROUNDS;
    use lil_poker_mccfr::game::holdem::RoundHistory;
    use std::collections::HashMap;
    let hole = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Queen, Suit::Hearts),
    ];
    let board_flop = [
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Two, Suit::Spades),
    ];
    let mut history = [RoundHistory::new(); NUM_ROUNDS];
    history[0].push(RAISE_MIN);
    history[0].push(CALL_CHECK);
    /* Flop facing a raise: [RAISE_MIN, RAISE_HALF_POT] -> "rh" */
    history[1].push(RAISE_MIN);
    history[1].push(RAISE_HALF_POT);

    let bucket = postflop_equity_bucket(&hole, &board_flop);
    let check_node_key = format!("P:rc|F:B{bucket}/");
    let mut map: HashMap<String, Vec<f64>> = HashMap::new();
    /* Only a check-node (0% fold) exists in the map; Tier-5 truncation must NOT return it for "rh"! */
    map.insert(check_node_key, vec![0.0, 0.90, 0.10, 0.0, 0.0, 0.0]);

    let found = lookup_strategy_hierarchical_owned(
        |k| map.get(k).cloned(),
        &hole,
        &board_flop,
        2,
        &history,
    );
    assert!(
        found.is_none(),
        "Facing-bet history 'rh' must NEVER truncate to check-node '' with 0% fold"
    );
}

#[test]
fn test_unified_single_model_multi_stack_keys_and_lookup() {
    use lil_poker_mccfr::cfr::abstraction::{
        hash_from_infoset_key_str, hash_holdem_infoset_key, mix_stack_prefix_hash,
        stack_depth_prefix,
    };
    use lil_poker_mccfr::cfr::fallback::lookup_strategy_hierarchical_owned_with_stack;
    use lil_poker_mccfr::game::config::NUM_ROUNDS;
    use lil_poker_mccfr::game::holdem::RoundHistory;
    use std::collections::HashMap;

    let hole = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Spades),
    ];
    let history = [RoundHistory::new(); NUM_ROUNDS];

    /* 1. Hash parity between incremental mix and full string parser for S15| / S30| / 100BB */
    let base_h = hash_holdem_infoset_key(&hole, &[], 1, &history);
    assert_eq!(
        mix_stack_prefix_hash(base_h, stack_depth_prefix(15)),
        hash_from_infoset_key_str("S15|P:AKs/")
    );
    assert_eq!(
        mix_stack_prefix_hash(base_h, stack_depth_prefix(30)),
        hash_from_infoset_key_str("S30|P:AKs/")
    );
    assert_eq!(
        mix_stack_prefix_hash(base_h, stack_depth_prefix(100)),
        hash_from_infoset_key_str("P:AKs/")
    );

    /* 2. Single HashMap serves 15 BB, 30 BB, and 100 BB keys + graceful fallback to 100 BB */
    let mut map: HashMap<String, Vec<f64>> = HashMap::new();
    map.insert("S15|P:AKs/".to_string(), vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    map.insert("P:AKs/".to_string(), vec![0.0, 0.1, 0.5, 0.4, 0.0, 0.0]);

    let p15 = lookup_strategy_hierarchical_owned_with_stack(
        |k| map.get(k).cloned(),
        &hole,
        &[],
        1,
        &history,
        15,
    )
    .expect("15 BB lookup");
    assert!((p15[ALL_IN as usize] - 1.0).abs() < 1e-6);

    let p30 = lookup_strategy_hierarchical_owned_with_stack(
        |k| map.get(k).cloned(),
        &hole,
        &[],
        1,
        &history,
        30,
    )
    .expect("30 BB falls back to 100 BB when S30| absent");
    assert!((p30[RAISE_MIN as usize] - 0.5).abs() < 1e-6);

    let p100 = lookup_strategy_hierarchical_owned_with_stack(
        |k| map.get(k).cloned(),
        &hole,
        &[],
        1,
        &history,
        100,
    )
    .expect("100 BB lookup");
    assert!((p100[RAISE_MIN as usize] - 0.5).abs() < 1e-6);
}

#[test]
fn test_v2_board_texture_keys_hash_parity_and_fallback() {
    use lil_poker_mccfr::cfr::abstraction::{
        append_texture_to_key, compact_board_texture_char, get_holdem_infoset_key_rich,
        hash_from_infoset_key_str, hash_holdem_infoset_key_rich, mix_stack_prefix_hash,
        mix_texture_hash, postflop_equity_bucket,
    };
    use lil_poker_mccfr::cfr::fallback::lookup_strategy_hierarchical_owned_with_stack;
    use lil_poker_mccfr::game::config::NUM_ROUNDS;
    use lil_poker_mccfr::game::holdem::RoundHistory;
    use std::collections::HashMap;

    let hole = [
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::King, Suit::Diamonds),
    ];
    let mono_board = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Four, Suit::Spades),
    ];
    let dry_board = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Nine, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    assert_eq!(compact_board_texture_char(&mono_board), 'm');
    assert_eq!(compact_board_texture_char(&dry_board), 'd');

    let mut history = [RoundHistory::new(); NUM_ROUNDS];
    history[0].push(RAISE_MIN);
    history[0].push(CALL_CHECK);

    /* 1. Hash parity for combined S15| + board texture 'm' key */
    let base_key = get_holdem_infoset_key_rich(&hole, &mono_board, 2, &history);
    let tex_key = format!("S15|{}", append_texture_to_key(&base_key, 'm'));
    let base_h = hash_holdem_infoset_key_rich(&hole, &mono_board, 2, &history);
    let expected_h = mix_stack_prefix_hash(mix_texture_hash(base_h, 'm'), "S15|");
    assert_eq!(
        hash_from_infoset_key_str(&tex_key),
        expected_h,
        "hash_from_infoset_key_str must match incremental stack+texture hash for {tex_key}"
    );

    /* 2. Lookup prefers textured key when present and falls back to untextured v1 key when absent */
    let b_mono = postflop_equity_bucket(&hole, &mono_board);
    let mut map: HashMap<String, Vec<f64>> = HashMap::new();
    map.insert(
        format!("S15|P:rc|F:B{b_mono:02}m/"),
        vec![0.0, 0.80, 0.20, 0.0, 0.0, 0.0],
    );
    map.insert(
        format!("P:rc|F:B{b_mono:02}/"),
        vec![0.0, 0.10, 0.0, 0.90, 0.0, 0.0],
    );

    let p_mono_15 = lookup_strategy_hierarchical_owned_with_stack(
        |k| map.get(k).cloned(),
        &hole,
        &mono_board,
        2,
        &history,
        15,
    )
    .expect("v2 textured 15 BB lookup");
    assert!((p_mono_15[CALL_CHECK as usize] - 0.80).abs() < 1e-6);

    let p_mono_100 = lookup_strategy_hierarchical_owned_with_stack(
        |k| map.get(k).cloned(),
        &hole,
        &mono_board,
        2,
        &history,
        100,
    )
    .expect("v1 untextured fallback lookup");
    assert!((p_mono_100[RAISE_THIRD_POT as usize] - 0.90).abs() < 1e-6);
}
