use lil_poker_mccfr::cfr::abstraction::{
    append_texture_to_key, compact_board_texture_char, compact_turn_texture_char,
    get_holdem_infoset_key_rich, get_holdem_infoset_key_rich_with_bucket,
    hash_from_infoset_key_str, hash_holdem_infoset_key, hash_holdem_infoset_key_rich,
    hash_holdem_infoset_key_rich_fast, mix_stack_prefix_hash, mix_texture_hash,
    postflop_equity_bucket, postflop_extended_bucket, preflop_bucket, preflop_strength_tier,
    stack_depth_prefix, stack_depth_prefix_four_tier,
};
use lil_poker_mccfr::cfr::fallback::{
    FallbackRequest, StrategyLookupIndex, get_holdem_fallback_strategy,
    lookup_strategy_hierarchical_owned, lookup_strategy_hierarchical_owned_with_stack,
    lookup_strategy_stack_interpolated,
};
use lil_poker_mccfr::cfr::holdem_mccfr::HoldemMCCFRSolver;
use lil_poker_mccfr::game::config::NUM_ROUNDS;
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, Card, FOLD, HOLDEM_NUM_ACTIONS, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
    RoundHistory,
};
use lil_poker_mccfr::live::parse_card_str;
use std::collections::HashMap;

const LEGAL_ALL: [u8; 6] = [
    FOLD,
    CALL_CHECK,
    RAISE_MIN,
    RAISE_THIRD_POT,
    RAISE_HALF_POT,
    ALL_IN,
];

fn card(s: &str) -> Card {
    parse_card_str(s).unwrap_or_else(|| panic!("invalid card token: {s}"))
}

fn hole(s: &str) -> [Card; 2] {
    let list: Vec<Card> = s.split_whitespace().map(card).collect();
    [list[0], list[1]]
}

fn board(s: &str) -> Vec<Card> {
    if s.trim().is_empty() {
        Vec::new()
    } else {
        s.split_whitespace().map(card).collect()
    }
}

fn fb_req<'a>(
    h: [Card; 2],
    b: &'a [Card],
    round: u8,
    to_call: i32,
    pot: i32,
    legal: &'a [u8],
) -> FallbackRequest<'a> {
    FallbackRequest {
        hole: h,
        board: b,
        round,
        to_call,
        pot,
        legal,
        ..Default::default()
    }
}

fn hist_pf_flop(pf: &[u8], flop: &[u8]) -> [RoundHistory; NUM_ROUNDS] {
    let mut history = [RoundHistory::new(); NUM_ROUNDS];
    for &a in pf {
        history[0].push(a);
    }
    for &a in flop {
        history[1].push(a);
    }
    history
}

fn lookup_stack(
    map: &HashMap<String, Vec<f64>>,
    h: [Card; 2],
    b: &[Card],
    round: u8,
    history: &[RoundHistory; NUM_ROUNDS],
    bb: i32,
) -> Option<Vec<f64>> {
    lookup_strategy_hierarchical_owned_with_stack(
        |k| map.get(k).cloned(),
        &h,
        b,
        round,
        history,
        bb,
    )
}

#[test]
fn test_fallback_strategy_preflop_and_postflop() {
    let raise_mass = |s: &[f64]| {
        s[RAISE_HALF_POT as usize]
            + s[RAISE_THIRD_POT as usize]
            + s[RAISE_MIN as usize]
            + s[ALL_IN as usize]
    };

    let strat_aa = get_holdem_fallback_strategy(&fb_req(hole("As Ah"), &[], 1, 20, 40, &LEGAL_ALL));
    assert_eq!(strat_aa[FOLD as usize], 0.0, "AA should never fold");
    assert!(raise_mass(&strat_aa) > 0.50, "AA should raise frequently");

    let hole_72 = hole("7d 2c");
    let strat_72 = get_holdem_fallback_strategy(&fb_req(hole_72, &[], 1, 20, 40, &LEGAL_ALL));
    let fold_72 = strat_72[FOLD as usize];
    assert!(fold_72 > 0.90, "72o should fold > 90%, got {fold_72}");

    let board_fl = board("8s 6s 2s Kh Qd");
    let strat_fl =
        get_holdem_fallback_strategy(&fb_req(hole("As Ts"), &board_fl, 4, 100, 300, &LEGAL_ALL));
    assert_eq!(strat_fl[FOLD as usize], 0.0, "Nut flush never folds");
    assert!(raise_mass(&strat_fl) > 0.50, "Nut flush raises for value");

    let board_air = board("Kh Qd 9s");
    let legal_free = [
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let strat_air =
        get_holdem_fallback_strategy(&fb_req(hole_72, &board_air, 2, 0, 100, &legal_free));
    assert_eq!(strat_air[FOLD as usize], 0.0);
    assert!(
        strat_air[CALL_CHECK as usize] >= 0.85,
        "Air should check >= 85%"
    );
}

#[test]
fn test_strategy_lookup_index() {
    let hole_aa = hole("As Ah");
    let mut map = HashMap::new();
    map.insert("P:AA/c".to_string(), vec![0.0, 0.2, 0.4, 0.2, 0.2, 0.0]);
    map.insert("P:AA/r".to_string(), vec![0.0, 0.0, 0.2, 0.4, 0.3, 0.1]);
    map.insert("F:B10/c".to_string(), vec![0.1, 0.5, 0.2, 0.1, 0.1, 0.0]);
    map.insert(
        "P:c|F:B10/r".to_string(),
        vec![0.1, 0.3, 0.2, 0.3, 0.1, 0.0],
    );

    let index = StrategyLookupIndex::build_from_map(&map);
    let pf_avg = index
        .query_bucket_avg(1, &hole_aa, &[], 0)
        .expect("pf bucket avg");
    for (i, expected) in [(0usize, 0.0), (1, 0.1), (2, 0.3), (3, 0.3)] {
        assert!((pf_avg[i] - expected).abs() < 1e-6);
    }

    let blended = index.get_blended_fallback(&fb_req(hole_aa, &[], 1, 20, 50, &LEGAL_ALL));
    assert_eq!(blended.len(), HOLDEM_NUM_ACTIONS);
    assert!((blended.iter().sum::<f64>() - 1.0).abs() < 1e-6);
    assert_eq!(blended[FOLD as usize], 0.0, "Pocket Aces should not fold");
}

#[test]
fn test_binary_strategy_save_and_load() {
    let solver = HoldemMCCFRSolver::new();
    let temp_path = "/tmp/test_binary_strategy.bin";
    solver.train(100, 1, 0);
    let count = solver
        .save_strategy_to_file(temp_path)
        .expect("Save binary failed");
    assert!(count > 0);
    assert_eq!(
        HoldemMCCFRSolver::load_strategy_from_file(temp_path)
            .expect("Load bin")
            .len(),
        count
    );
    assert_eq!(
        lil_poker_mccfr::sim::holdem::load_holdem_strategy(temp_path)
            .expect("Load sim")
            .len(),
        count
    );
    let _ = std::fs::remove_file(temp_path);
}

#[test]
fn test_preflop_strength_tiers_and_88_shove_defense() {
    for (h, tier) in [
        ("As Ah", 1),
        ("As Ks", 1),
        ("As Kh", 1),
        ("Js Jh", 2),
        ("As Qs", 2),
        ("As Qh", 2),
        ("8s 8h", 3),
        ("As Jh", 3),
        ("2s 2h", 5),
        ("7d 2c", 8),
    ] {
        let c = hole(h);
        assert_eq!(preflop_strength_tier(preflop_bucket(c[0], c[1]).0), tier);
    }

    let legal = [FOLD, CALL_CHECK, ALL_IN];
    let strat_88 = get_holdem_fallback_strategy(&fb_req(hole("8s 8h"), &[], 1, 4000, 8000, &legal));
    let fold_88 = strat_88[FOLD as usize];
    assert!(
        fold_88 >= 0.70,
        "88 facing 40 BB shove must fold >= 70%, got {fold_88}"
    );
}

#[test]
fn test_fallback_high_blinds_and_turn_owned_lookup() {
    let (hole_44, legal) = (hole("4s 4h"), [FOLD, CALL_CHECK, RAISE_MIN]);
    let strat_bb400 = get_holdem_fallback_strategy(&FallbackRequest {
        table_bb: 400,
        ..fb_req(hole_44, &[], 1, 400, 1000, &legal)
    });
    let strat_bb100 = get_holdem_fallback_strategy(&FallbackRequest {
        table_bb: 100,
        ..fb_req(hole_44, &[], 1, 400, 1000, &legal)
    });
    assert!(strat_bb400[CALL_CHECK as usize] > strat_bb100[CALL_CHECK as usize] + 0.2);

    let (hole_ak, board_turn) = (hole("As Kh"), board("Ad 9c 4s 2c"));
    let history = hist_pf_flop(&[RAISE_MIN, CALL_CHECK], &[CALL_CHECK, RAISE_MIN]);
    let bucket = postflop_equity_bucket(&hole_ak, &board_turn);
    let mut map: HashMap<String, Vec<f64>> = HashMap::new();
    map.insert(
        format!("P:rc|F:cc|T:B{bucket}/"),
        vec![0.0, 0.2, 0.3, 0.3, 0.2, 0.0],
    );
    let found = lookup_strategy_hierarchical_owned(
        |k| map.get(k).cloned(),
        &hole_ak,
        &board_turn,
        3,
        &history,
    );
    assert!(found.is_some());
}

#[test]
fn test_tier5_truncation_preserves_facing_bet_parity() {
    let (h, b) = (hole("Ks Qh"), board("Kd 7c 2s"));
    let history = hist_pf_flop(&[RAISE_MIN, CALL_CHECK], &[RAISE_MIN, RAISE_HALF_POT]);
    let bucket = postflop_equity_bucket(&h, &b);
    let mut map: HashMap<String, Vec<f64>> = HashMap::new();
    map.insert(
        format!("P:rc|F:B{bucket}/"),
        vec![0.0, 0.90, 0.10, 0.0, 0.0, 0.0],
    );
    assert!(
        lookup_strategy_hierarchical_owned(|k| map.get(k).cloned(), &h, &b, 2, &history).is_none(),
        "Facing-bet history 'rh' must NEVER truncate to check-node ''"
    );
}

#[test]
fn test_unified_single_model_multi_stack_keys_and_lookup() {
    let (h, history) = (hole("As Ks"), [RoundHistory::new(); NUM_ROUNDS]);
    let base_h = hash_holdem_infoset_key(&h, &[], 1, &history);
    for (bb, key) in [(15, "S15|P:AKs/"), (30, "S30|P:AKs/"), (100, "P:AKs/")] {
        assert_eq!(
            mix_stack_prefix_hash(base_h, stack_depth_prefix(bb)),
            hash_from_infoset_key_str(key)
        );
    }

    let mut map: HashMap<String, Vec<f64>> = HashMap::new();
    map.insert("S15|P:AKs/".to_string(), vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    map.insert("P:AKs/".to_string(), vec![0.0, 0.1, 0.5, 0.4, 0.0, 0.0]);

    for (bb, idx, expected, msg) in [
        (15, ALL_IN as usize, 1.0, "15 BB lookup"),
        (30, RAISE_MIN as usize, 0.5, "30 BB fallback to 100 BB"),
        (100, RAISE_MIN as usize, 0.5, "100 BB lookup"),
    ] {
        let p = lookup_stack(&map, h, &[], 1, &history, bb).expect(msg);
        assert!((p[idx] - expected).abs() < 1e-6);
    }
}

#[test]
fn test_board_texture_keys_hash_parity_and_fallback() {
    let (h, mono_board, dry_board) = (hole("Ac Kd"), board("As 9s 4s"), board("As 9d 2c"));
    assert_eq!(compact_board_texture_char(&mono_board), 'm');
    assert_eq!(compact_board_texture_char(&dry_board), 'd');

    let history = hist_pf_flop(&[RAISE_MIN, CALL_CHECK], &[]);
    let base_key = get_holdem_infoset_key_rich(&h, &mono_board, 2, &history);
    let tex_key = format!("S15|{}", append_texture_to_key(&base_key, 'm'));
    let base_h = hash_holdem_infoset_key_rich(&h, &mono_board, 2, &history);
    let expected_h = mix_stack_prefix_hash(mix_texture_hash(base_h, 'm'), "S15|");
    assert_eq!(
        hash_from_infoset_key_str(&tex_key),
        expected_h,
        "hash mismatch {tex_key}"
    );

    let b_mono = postflop_equity_bucket(&h, &mono_board);
    let mut map: HashMap<String, Vec<f64>> = HashMap::new();
    map.insert(
        format!("S15|P:rc|F:B{b_mono:02}m/"),
        vec![0.0, 0.80, 0.20, 0.0, 0.0, 0.0],
    );
    map.insert(
        format!("P:rc|F:B{b_mono:02}/"),
        vec![0.0, 0.10, 0.0, 0.90, 0.0, 0.0],
    );

    let p15 = lookup_stack(&map, h, &mono_board, 2, &history, 15).expect("15 BB textured");
    assert!((p15[CALL_CHECK as usize] - 0.80).abs() < 1e-6);
    let p100 = lookup_stack(&map, h, &mono_board, 2, &history, 100).expect("100 BB fallback");
    assert!((p100[RAISE_THIRD_POT as usize] - 0.90).abs() < 1e-6);
}

#[test]
fn test_turn_texture_stacks_hash_parity_and_three_tier_compatibility() {
    assert_eq!(compact_turn_texture_char(&board("Ks 8d 3c 2h")), 'c');
    assert_eq!(compact_turn_texture_char(&board("Ks 8d 8c 2h")), 'p');
    assert_eq!(compact_turn_texture_char(&board("Ks Qs 4s 2h")), 'x');
    for (bb, prefix) in [(15, "S15|"), (25, "S25|"), (45, "S45|"), (100, "")] {
        assert_eq!(stack_depth_prefix_four_tier(bb), prefix);
    }

    let (h, turn_board) = (hole("As Ks"), board("Kd 9s 4s 2c"));
    let b_ext = postflop_extended_bucket(&h, &turn_board) as u8;
    assert_eq!(
        (b_ext, postflop_equity_bucket(&h, &turn_board) as u8),
        (109, 49)
    );

    let history = hist_pf_flop(&[RAISE_MIN, CALL_CHECK], &[CALL_CHECK, CALL_CHECK]);
    let turn_tex = compact_turn_texture_char(&turn_board);
    let base_key = get_holdem_infoset_key_rich_with_bucket(&h, b_ext, 3, &history);
    let ext_key = format!("S25|{}", append_texture_to_key(&base_key, turn_tex));
    let fast_h = hash_holdem_infoset_key_rich_fast(b_ext, 3, &history);
    let expected_h = mix_stack_prefix_hash(mix_texture_hash(fast_h, turn_tex), "S25|");
    assert_eq!(
        hash_from_infoset_key_str(&ext_key),
        expected_h,
        "hash mismatch {ext_key}"
    );

    let mut three_tier_map: HashMap<String, Vec<f64>> = HashMap::new();
    three_tier_map.insert(
        "S30|P:rc|F:cc|T:B49/".to_string(),
        vec![0.0, 0.25, 0.75, 0.0, 0.0, 0.0],
    );
    let p_3tier = lookup_stack(&three_tier_map, h, &turn_board, 3, &history, 25).expect("3-tier");
    assert!((p_3tier[RAISE_MIN as usize] - 0.75).abs() < 1e-6);

    let mut ext_map: HashMap<String, Vec<f64>> = HashMap::new();
    ext_map.insert(ext_key, vec![0.0, 0.10, 0.0, 0.90, 0.0, 0.0]);
    ext_map.insert(
        format!("S45|{}", append_texture_to_key(&base_key, turn_tex)),
        vec![0.0, 0.50, 0.0, 0.50, 0.0, 0.0],
    );
    let p_ext = lookup_stack(&ext_map, h, &turn_board, 3, &history, 25).expect("S25| + B109c");
    assert!((p_ext[RAISE_THIRD_POT as usize] - 0.90).abs() < 1e-6);

    let p_interp =
        lookup_strategy_stack_interpolated(|k| ext_map.get(k), &h, &turn_board, 3, &history, 35)
            .expect("4-tier stack interpolation at 35 BB");
    assert!((p_interp[RAISE_THIRD_POT as usize] - 0.70).abs() < 1e-6);

    let idx = StrategyLookupIndex::build_from_map(&ext_map);
    let avg = idx
        .query_bucket_avg(3, &h, &turn_board, 0)
        .expect("3-digit bucket query");
    assert!((avg[RAISE_THIRD_POT as usize] - 0.70).abs() < 1e-6);
}
