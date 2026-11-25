use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, Rank, Suit,
    calculate_geometric_bet_size,
};
use lil_poker_mccfr::live::protocol::{
    ActionMapRequest, map_action_index, parse_card_str, parse_json_i32,
    pseudo_harmonic_action_mapping, pseudo_harmonic_action_weights, pseudo_harmonic_prob,
};
use rand::SeedableRng;
use serde_json::json;

#[test]
fn test_live_parse_card_str() {
    let ace_spades = parse_card_str("As").expect("parse As");
    assert_eq!(ace_spades.rank, Rank::Ace);
    assert_eq!(ace_spades.suit, Suit::Spades);

    let ten_hearts = parse_card_str("10h").expect("parse 10h");
    assert_eq!(ten_hearts.rank, Rank::Ten);
    assert_eq!(ten_hearts.suit, Suit::Hearts);

    let ten_diamonds = parse_card_str("Td").expect("parse Td");
    assert_eq!(ten_diamonds.rank, Rank::Ten);
    assert_eq!(ten_diamonds.suit, Suit::Diamonds);

    let king_spades_unicode = parse_card_str("K♠").expect("parse unicode K♠");
    assert_eq!(king_spades_unicode.rank, Rank::King);
    assert_eq!(king_spades_unicode.suit, Suit::Spades);

    let quoted_queen_clubs = parse_card_str("'Q♣'").expect("parse quoted 'Q♣'");
    assert_eq!(quoted_queen_clubs.rank, Rank::Queen);
    assert_eq!(quoted_queen_clubs.suit, Suit::Clubs);

    assert!(parse_card_str("").is_none());
    assert!(parse_card_str("Xx").is_none());
}

#[test]
fn test_live_parse_json_i32() {
    let i = json!(42);
    assert_eq!(parse_json_i32(Some(&i)), 42);

    let f = json!(123.0);
    assert_eq!(parse_json_i32(Some(&f)), 123);

    let s = json!("500");
    assert_eq!(parse_json_i32(Some(&s)), 500);

    assert_eq!(parse_json_i32(None), 0);
}

#[test]
fn test_live_map_action_index() {
    let legal = vec![
        "fold".to_string(),
        "check".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];

    /* Index 0 (Fold): Facing no bet (to_call == 0) -> should prefer check! */
    let (act_free, amt_free) = map_action_index(&ActionMapRequest {
        action_idx: FOLD as usize,
        legal: &legal,
        to_call: 0,
        pot: 100,
        ..Default::default()
    })
    .expect("map index 0 when free");
    assert_eq!(act_free, "check");
    assert_eq!(amt_free, 0);

    /* Index 0 (Fold): Facing bet (to_call > 0) -> should fold */
    let (act_facing_bet, _) = map_action_index(&ActionMapRequest {
        action_idx: FOLD as usize,
        legal: &legal,
        to_call: 50,
        pot: 100,
        ..Default::default()
    })
    .expect("map index 0 facing bet");
    assert_eq!(act_facing_bet, "fold");

    /* Index 1 (Call / Check): When to_call > 0 -> call, when to_call == 0 -> check */
    let (act_call, amt_call) = map_action_index(&ActionMapRequest {
        action_idx: CALL_CHECK as usize,
        legal: &legal,
        to_call: 50,
        pot: 100,
        ..Default::default()
    })
    .expect("map index 1 facing bet");
    assert_eq!(act_call, "call", "Facing bet must map to call, not check!");
    assert_eq!(amt_call, 0);

    let (act_check, amt_check) = map_action_index(&ActionMapRequest {
        action_idx: CALL_CHECK as usize,
        legal: &legal,
        to_call: 0,
        pot: 150,
        round: 2,
        ..Default::default()
    })
    .expect("map index 1 when free");
    assert_eq!(
        act_check, "check",
        "Free action must map to check, not call!"
    );
    assert_eq!(amt_check, 0);

    /* Index 4 (Raise Half Pot): dry vs wet board */
    let (_, amt_dry) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_HALF_POT as usize,
        legal: &legal,
        pot: 1000,
        is_wet: false,
        ..Default::default()
    })
    .expect("raise half dry");
    let (_, amt_wet) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_HALF_POT as usize,
        legal: &legal,
        pot: 1000,
        is_wet: true,
        ..Default::default()
    })
    .expect("raise half wet");
    assert!(
        amt_wet > amt_dry,
        "Wet board raise half-pot should size larger than dry board"
    );

    /* Index 5 (All-in) */
    let (act_allin, _) = map_action_index(&ActionMapRequest {
        action_idx: ALL_IN as usize,
        legal: &legal,
        to_call: 50,
        pot: 100,
        ..Default::default()
    })
    .expect("allin");
    assert_eq!(act_allin, "allin");
}

#[test]
fn test_live_map_action_index_round() {
    let legal = vec![
        "fold".to_string(),
        "check".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];

    /* On River (round 4), RAISE_HALF_POT should be polarized 150% pot overbet (1500 chips) */
    let (_, amt_river) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_HALF_POT as usize,
        legal: &legal,
        pot: 1000,
        round: 4,
        ..Default::default()
    })
    .expect("river bet sizing");
    assert_eq!(
        amt_river, 1500,
        "River bet sizing must scale to 150% pot overbet"
    );

    /* On Turn (round 3), RAISE_HALF_POT should be polarized 150% pot overbet (1500 chips) */
    let (_, amt_turn) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_HALF_POT as usize,
        legal: &legal,
        pot: 1000,
        round: 3,
        ..Default::default()
    })
    .expect("turn bet sizing");
    assert_eq!(
        amt_turn, 1500,
        "Turn bet sizing must scale to 150% pot overbet"
    );

    /* On Flop (round 2): 33% RAISE_MIN (333), 60% RAISE_THIRD_POT (600), 75% dry (750), 100% wet (1000) */
    let (_, amt_flop_33) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_MIN as usize,
        legal: &legal,
        pot: 1000,
        round: 2,
        ..Default::default()
    })
    .expect("flop 33% sizing");
    assert_eq!(amt_flop_33, 333);

    let (_, amt_flop_60) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_THIRD_POT as usize,
        legal: &legal,
        pot: 1000,
        round: 2,
        ..Default::default()
    })
    .expect("flop 60% sizing");
    assert_eq!(amt_flop_60, 600);

    let (_, amt_flop_dry) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_HALF_POT as usize,
        legal: &legal,
        pot: 1000,
        round: 2,
        is_wet: false,
        ..Default::default()
    })
    .expect("flop dry 75% sizing");
    let (_, amt_flop_wet) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_HALF_POT as usize,
        legal: &legal,
        pot: 1000,
        round: 2,
        is_wet: true,
        ..Default::default()
    })
    .expect("flop wet 100% sizing");
    assert_eq!(amt_flop_dry, 750);
    assert_eq!(amt_flop_wet, 1000);

    /* HU Preflop 3.5x 3-bet against 250 open (100 BB stack) -> 875 total */
    let (_, hu_3bet_amt) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_THIRD_POT as usize,
        legal: &legal,
        to_call: 150,
        pot: 350,
        round: 1,
        table_bb: 100,
        my_chips: 9900,
        current_bet: 250,
        ..Default::default()
    })
    .expect("HU 3.5x 3bet");
    assert_eq!(hu_3bet_amt, 875);
}

#[test]
fn test_pseudo_harmonic_and_geometric_sizing() {
    /* 1. Geometric bet sizing commits stack smoothly */
    let pot = 1000;
    let stack = 4000;
    let bet_2streets = calculate_geometric_bet_size(pot, stack, 2);
    assert!(
        bet_2streets > 500 && bet_2streets < stack,
        "Geometric sizing should be reasonable fraction of pot, got {bet_2streets}"
    );

    /* 2. Pseudo-Harmonic probability boundary conditions */
    assert_eq!(pseudo_harmonic_prob(100, 100, 300, 1000), 1.0);
    assert_eq!(pseudo_harmonic_prob(300, 100, 300, 1000), 0.0);
    let p_mid = pseudo_harmonic_prob(200, 100, 300, 1000);
    assert!(p_mid > 0.0 && p_mid < 1.0);

    /* 3. Action mapping preserves bounds */
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);
    let act_min = pseudo_harmonic_action_mapping(100, 1000, 8000, &mut rng);
    assert_eq!(act_min, RAISE_MIN, "Small bet must map to min-raise");

    let act_shove = pseudo_harmonic_action_mapping(8500, 1000, 8000, &mut rng);
    assert_eq!(act_shove, ALL_IN, "Huge bet must map to all-in");
}

#[test]
fn test_protocol_6max_limp_pot_and_multiway_3bet_sizing() {
    let legal = vec![
        "fold".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];

    /* 1. 6-max limped pot: bb=400, current_bet=400, to_call=400, pot=1800 -> ISO open 1400 (3.5 BB), not 3-bet bloat */
    let (_, limp_iso_amt) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_HALF_POT as usize,
        legal: &legal,
        to_call: 400,
        pot: 1800,
        is_wet: false,
        round: 1,
        table_bb: 400,
        my_chips: 20000,
        current_bet: 400,
    })
    .expect("map limp iso");
    assert_eq!(
        limp_iso_amt, 1400,
        "Limped 6-max ISO raise should be 3.5x BB (1400), not bloated by multiway limps"
    );

    /* 2. Multiway 3-bet: bb=400, open current_bet=1200 + 2 callers -> pot=4200, to_call=1200 -> 3.0x of 1200 = 3600 */
    let (_, mw_3bet_amt) = map_action_index(&ActionMapRequest {
        action_idx: RAISE_HALF_POT as usize,
        legal: &legal,
        to_call: 1200,
        pot: 4200,
        is_wet: false,
        round: 1,
        table_bb: 400,
        my_chips: 20000,
        current_bet: 1200,
    })
    .expect("map multiway 3bet");
    assert_eq!(
        mw_3bet_amt, 3600,
        "Multiway 3-bet vs 1200 open must scale off current_bet=1200 (3600), not inflated pot"
    );

    /* 3. Compact 2.2x preflop 4-bet sizing when opp_bet_pf > 5 BB (1400 -> 3080, not 3850) */
    let (_, four_bet_amt) = map_action_index(&ActionMapRequest {
        action_idx: lil_poker_mccfr::game::holdem::RAISE_THIRD_POT as usize,
        legal: &legal,
        to_call: 1150,
        pot: 1650,
        is_wet: false,
        round: 1,
        table_bb: 100,
        my_chips: 10600,
        current_bet: 1400,
    })
    .expect("map 4-bet");
    assert_eq!(
        four_bet_amt, 3080,
        "Preflop 4-bet vs 1400 (14 BB) must size compactly at 2.2x (3080), got {four_bet_amt}"
    );
}

#[test]
fn test_sb_150_300_allin_never_leaves_one_chip() {
    let legal_with_allin = vec![
        "fold".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "all_in".to_string(),
    ];
    let legal_raise_only = vec!["fold".to_string(), "call".to_string(), "raise".to_string()];

    for (bb, sb, stack, expected_total) in [
        (300, 150, 4381, 4531),
        (400, 200, 1761, 1961),
        (600, 300, 4364, 4664),
    ] {
        let (act, amt) = map_action_index(&ActionMapRequest {
            action_idx: ALL_IN as usize,
            legal: &legal_with_allin,
            to_call: sb,
            pot: bb + sb,
            is_wet: false,
            round: 1,
            table_bb: bb,
            my_chips: stack,
            current_bet: bb,
        })
        .expect("map SB all_in");
        assert_eq!(act, "all_in");
        assert_eq!(amt, 0);

        let (raise_act, raise_amt) = map_action_index(&ActionMapRequest {
            action_idx: ALL_IN as usize,
            legal: &legal_raise_only,
            to_call: sb,
            pot: bb + sb,
            is_wet: false,
            round: 1,
            table_bb: bb,
            my_chips: stack,
            current_bet: bb,
        })
        .expect("map SB raise fallback for full stack");
        assert_eq!(raise_act, "raise");
        assert_eq!(raise_amt, expected_total);
    }
}

#[test]
fn test_pseudo_harmonic_weights_smooth_overbet_interpolation() {
    /* Smooth weight transitions across RAISE_MIN -> RAISE_THIRD_POT -> RAISE_HALF_POT -> ALL_IN */
    let b_mid = pseudo_harmonic_action_weights(450, 1000, 8000);
    assert_eq!(b_mid.low_action, RAISE_MIN);
    assert_eq!(b_mid.high_action, RAISE_THIRD_POT);
    assert!(b_mid.low_weight > 0.0 && b_mid.low_weight < 1.0);

    let b_overbet = pseudo_harmonic_action_weights(1500, 1000, 8000);
    assert_eq!(b_overbet.low_action, RAISE_HALF_POT);
    assert_eq!(b_overbet.high_action, ALL_IN);
    assert!(
        b_overbet.low_weight > 0.1 && b_overbet.low_weight < 0.99,
        "150% pot overbet below allin threshold must smoothly interpolate between RAISE_HALF_POT and ALL_IN, got {b_overbet:?}"
    );

    let b_bigger_overbet = pseudo_harmonic_action_weights(4500, 1000, 8000);
    assert_eq!(b_bigger_overbet.low_action, RAISE_HALF_POT);
    assert_eq!(b_bigger_overbet.high_action, ALL_IN);
    assert!(
        b_bigger_overbet.low_weight < b_overbet.low_weight,
        "Larger overbet must shift weight monotonically toward ALL_IN"
    );
}
