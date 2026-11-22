use super::{LEGAL_FULL, LEGAL_OPEN, make_aggro_maniac, make_overbet_bluffer, make_ultra_nit};
use lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket;
use lil_poker_mccfr::cfr::opponent_model::{OpponentTracker, PolicyRequest};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, Card, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, Rank, Suit,
};
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[test]
fn test_subgame_river_paired_defense_neutral_and_ultra_nit() {
    let mut rng = SmallRng::seed_from_u64(20_261_119);
    let tracker = OpponentTracker::new();
    let ultra_nit = make_ultra_nit();
    let jam_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.25, 0.0, 0.10, 0.10, 0.50];

    /* Js Ts on 5s Jc 8d 9s 5d paired connected River: neutral never jams, ultra-nit folds */
    let hole_jt = [
        Card::new(Rank::Jack, Suit::Spades),
        Card::new(Rank::Ten, Suit::Spades),
    ];
    let board_paired_danger_river = [
        Card::new(Rank::Five, Suit::Spades),
        Card::new(Rank::Jack, Suit::Clubs),
        Card::new(Rank::Eight, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Five, Suit::Diamonds),
    ];
    let b_jt = postflop_equity_bucket(&hole_jt, &board_paired_danger_river);
    let req_jt = PolicyRequest {
        raw_probs: &jam_heavy,
        legal_actions: &LEGAL_FULL,
        bucket: b_jt,
        round: 4,
        to_call: 5625,
        pot: 13125,
        hole: Some(&hole_jt),
        board: Some(&board_paired_danger_river),
        table_bb: 300,
        my_chips: 6359,
        from_subgame: true,
        ..Default::default()
    };
    let (act_neutral, _) = tracker.select_action_traced(&req_jt, &mut rng);
    assert!(
        act_neutral < RAISE_MIN,
        "Js Ts on paired dangerous river must never jam/raise, got {act_neutral}"
    );
    let (act_nit, _) = ultra_nit.select_action_traced(&req_jt, &mut rng);
    assert_eq!(
        act_nit, FOLD,
        "Js Ts vs ultra-nit triple barrel on paired dangerous river must FOLD"
    );
}

#[test]
fn test_subgame_four_flush_and_ultra_nit_river_folds() {
    let mut rng = SmallRng::seed_from_u64(20_261_119);
    let tracker = OpponentTracker::new();
    let ultra_nit = make_ultra_nit();
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.90, 0.05, 0.0, 0.0, 0.0];

    /* Qc Jc (no diamond) on 4-diamond river facing 375 into 1125 -> FOLD */
    let hole_qcjc = [
        Card::new(Rank::Queen, Suit::Clubs),
        Card::new(Rank::Jack, Suit::Clubs),
    ];
    let board_four_flush = [
        Card::new(Rank::Queen, Suit::Diamonds),
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Diamonds),
        Card::new(Rank::Four, Suit::Diamonds),
        Card::new(Rank::Five, Suit::Spades),
    ];
    let b_four_flush = postflop_equity_bucket(&hole_qcjc, &board_four_flush);
    let (act_four_flush, _) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_four_flush,
            round: 4,
            to_call: 375,
            pot: 1125,
            hole: Some(&hole_qcjc),
            board: Some(&board_four_flush),
            table_bb: 150,
            my_chips: 6256,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_four_flush, FOLD,
        "Qc Jc without diamond on 4-diamond river must fold"
    );

    /* Qh Js (3rd pair) on paired Ace river vs ultra-nit triple barrel -> FOLD */
    let hole_qj = [
        Card::new(Rank::Queen, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Spades),
    ];
    let board_paired_ace_river = [
        Card::new(Rank::Three, Suit::Clubs),
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Queen, Suit::Clubs),
        Card::new(Rank::Three, Suit::Spades),
        Card::new(Rank::Ace, Suit::Clubs),
    ];
    let b_qj = postflop_equity_bucket(&hole_qj, &board_paired_ace_river);
    let (act_third_pair_nit, _) = ultra_nit.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_qj,
            round: 4,
            to_call: 3750,
            pot: 11250,
            hole: Some(&hole_qj),
            board: Some(&board_paired_ace_river),
            table_bb: 600,
            my_chips: 8586,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_third_pair_nit, FOLD,
        "3rd pair Qh Js vs ultra-nit 3rd barrel must fold"
    );
}

#[test]
fn test_subgame_extreme_allin_overbet_cap() {
    let mut rng = SmallRng::seed_from_u64(20_261_119);
    let tracker = OpponentTracker::new();
    let raw_allin: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.05, 0.0, 0.05, 0.0, 0.90];

    let hole_4dtd = [
        Card::new(Rank::Four, Suit::Diamonds),
        Card::new(Rank::Ten, Suit::Diamonds),
    ];
    let board_turn_flush = [
        Card::new(Rank::Ace, Suit::Diamonds),
        Card::new(Rank::Three, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Diamonds),
        Card::new(Rank::Seven, Suit::Diamonds),
    ];
    let b_turn_flush = postflop_equity_bucket(&hole_4dtd, &board_turn_flush);
    let (act_turn_flush, _) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &raw_allin,
            legal_actions: &LEGAL_OPEN,
            bucket: b_turn_flush,
            round: 3,
            pot: 400,
            hole: Some(&hole_4dtd),
            board: Some(&board_turn_flush),
            my_chips: 9300,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_ne!(
        act_turn_flush, ALL_IN,
        "Extreme >20x pot Turn flush ALL_IN must be capped into standard value sizing"
    );
}

#[test]
fn test_subgame_river_showdown_and_paired_air_check_back() {
    let mut rng = SmallRng::seed_from_u64(20_261_119);
    let tracker = OpponentTracker::new();

    let hole_k5 = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Five, Suit::Hearts),
    ];
    let board_paired_khigh = [
        Card::new(Rank::Ten, Suit::Diamonds),
        Card::new(Rank::Eight, Suit::Clubs),
        Card::new(Rank::Eight, Suit::Hearts),
        Card::new(Rank::Two, Suit::Diamonds),
        Card::new(Rank::Four, Suit::Hearts),
    ];
    let b_khigh = postflop_equity_bucket(&hole_k5, &board_paired_khigh);
    let overbet_raw: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.10, 0.0, 0.0, 0.90, 0.0];
    let (act_khigh, _) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &overbet_raw,
            legal_actions: &LEGAL_OPEN,
            bucket: b_khigh,
            round: 4,
            pot: 500,
            hole: Some(&hole_k5),
            board: Some(&board_paired_khigh),
            my_chips: 12253,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_khigh, CALL_CHECK,
        "King-high showdown value must check back instead of 125% overbet bluff"
    );

    let hole_q9c = [
        Card::new(Rank::Queen, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Clubs),
    ];
    let board_paired_air_river = [
        Card::new(Rank::Jack, Suit::Diamonds),
        Card::new(Rank::Four, Suit::Hearts),
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::Ten, Suit::Clubs),
        Card::new(Rank::Ten, Suit::Spades),
    ];
    let b_paired_air = postflop_equity_bucket(&hole_q9c, &board_paired_air_river);
    let min_bluff_raw: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.15, 0.85, 0.0, 0.0, 0.0];
    let (act_paired_air, _) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &min_bluff_raw,
            legal_actions: &LEGAL_OPEN,
            bucket: b_paired_air,
            round: 4,
            pot: 1250,
            hole: Some(&hole_q9c),
            board: Some(&board_paired_air_river),
            my_chips: 9663,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_paired_air, CALL_CHECK,
        "Pure air must check back instead of 25% micro-bluff into paired river"
    );
}

#[test]
fn test_subgame_nut_straight_value_floor() {
    let mut rng = SmallRng::seed_from_u64(20_261_119);
    let tracker = OpponentTracker::new();
    let passive_check: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.85, 0.0, 0.15, 0.0, 0.0];

    let hole_kd8c = [
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Eight, Suit::Clubs),
    ];
    let board_four_straight_nut = [
        Card::new(Rank::Two, Suit::Hearts),
        Card::new(Rank::Five, Suit::Spades),
        Card::new(Rank::Four, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Six, Suit::Diamonds),
    ];
    let b_nut_straight = postflop_equity_bucket(&hole_kd8c, &board_four_straight_nut);
    let mut nut_straight_bets = 0usize;
    for _ in 0..80 {
        let (act, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &passive_check,
                legal_actions: &LEGAL_OPEN,
                bucket: b_nut_straight,
                round: 4,
                pot: 400,
                hole: Some(&hole_kd8c),
                board: Some(&board_four_straight_nut),
                table_bb: 200,
                my_chips: 5189,
                from_subgame: true,
                ..Default::default()
            },
            &mut rng,
        );
        if act >= RAISE_MIN {
            nut_straight_bets += 1;
        }
    }
    assert!(
        nut_straight_bets >= 45,
        "Nut Straight must value-bet majority of the time, got {nut_straight_bets}/80"
    );
}

#[test]
fn test_subgame_overbet_bluffer_bluff_catch() {
    let mut rng = SmallRng::seed_from_u64(20_261_119);
    let overbet_villain = make_overbet_bluffer();
    assert!(overbet_villain.is_overbet_bluffer());

    let hole_k7 = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Seven, Suit::Diamonds),
    ];
    let board_paired_overbet_river = [
        Card::new(Rank::Three, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Clubs),
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Spades),
        Card::new(Rank::Jack, Suit::Hearts),
    ];
    let b_bluff_catch = postflop_equity_bucket(&hole_k7, &board_paired_overbet_river);
    let fold_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.85, 0.15, 0.0, 0.0, 0.0, 0.0];
    let mut bluff_catch_calls = 0usize;
    for _ in 0..100 {
        let (act, _) = overbet_villain.select_action_traced(
            &PolicyRequest {
                raw_probs: &fold_heavy,
                legal_actions: &LEGAL_FULL,
                bucket: b_bluff_catch,
                round: 4,
                to_call: 4500,
                pot: 7500,
                hole: Some(&hole_k7),
                board: Some(&board_paired_overbet_river),
                table_bb: 150,
                my_chips: 5400,
                from_subgame: true,
                ..Default::default()
            },
            &mut rng,
        );
        if act == CALL_CHECK {
            bluff_catch_calls += 1;
        }
    }
    assert!(
        bluff_catch_calls >= 45,
        "Ks 7d bluff-catcher vs overbet spammer must call frequently, got {bluff_catch_calls}/100"
    );
}

#[test]
fn test_subgame_turn_and_river_one_pair_shove_folds() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_120);
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];

    let hole_a4o = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Four, Suit::Clubs),
    ];
    let board_turn_overbet_jam = [
        Card::new(Rank::Five, Suit::Spades),
        Card::new(Rank::Six, Suit::Clubs),
        Card::new(Rank::Two, Suit::Diamonds),
        Card::new(Rank::Ace, Suit::Hearts),
    ];
    let b_a4o = postflop_equity_bucket(&hole_a4o, &board_turn_overbet_jam);
    let (act_a4o, tr_a4o) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_a4o,
            round: 3,
            to_call: 8314,
            pot: 9010,
            hole: Some(&hole_a4o),
            board: Some(&board_turn_overbet_jam),
            table_bb: 100,
            my_chips: 8314,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_a4o, FOLD,
        "As 4c must fold to 11.9x pot turn jam in subgame (reasons: {:?})",
        tr_a4o.guard_reasons
    );

    let hole_qq = [
        Card::new(Rank::Queen, Suit::Hearts),
        Card::new(Rank::Queen, Suit::Diamonds),
    ];
    let board_paired_king_river = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Six, Suit::Clubs),
        Card::new(Rank::Five, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Spades),
    ];
    let b_qq = postflop_equity_bucket(&hole_qq, &board_paired_king_river);
    let (act_qq, tr_qq) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_qq,
            round: 4,
            to_call: 4648,
            pot: 9938,
            hole: Some(&hole_qq),
            board: Some(&board_paired_king_river),
            table_bb: 200,
            my_chips: 4648,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_qq, FOLD,
        "Qh Qd underpair on Ks 9h 6c 5h 9s must fold to river jam in subgame (reasons: {:?})",
        tr_qq.guard_reasons
    );

    /* Dry short-SPR river 2nd-pair facing ~82% pot all-in (pot_odds ~31%, <10 BB) must NOT auto-fold */
    let hole_8d6h = [
        Card::new(Rank::Eight, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Hearts),
    ];
    let board_dry_river = [
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Five, Suit::Spades),
        Card::new(Rank::Eight, Suit::Spades),
        Card::new(Rank::Two, Suit::Diamonds),
        Card::new(Rank::Queen, Suit::Clubs),
    ];
    let b_86 = postflop_equity_bucket(&hole_8d6h, &board_dry_river);
    assert_eq!(b_86, 35);
    let (act_86, tr_86) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_86,
            round: 4,
            to_call: 1961,
            pot: 4361,
            hole: Some(&hole_8d6h),
            board: Some(&board_dry_river),
            table_bb: 300,
            my_chips: 1961,
            opp_chips: Some(0),
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert!(
        act_86 != FOLD && !tr_86.guard_reasons.contains(&"SHOVE_DEFENSE_FOLD"),
        "Dry short-SPR river 2nd pair facing 82% pot bet must not auto-fold (reasons: {:?})",
        tr_86.guard_reasons
    );

    /* Turn paired board weak/mid underpair facing heavy commitment barrel (~29% odds, >40% stack) must fold */
    let hole_7k = [
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::King, Suit::Spades),
    ];
    let board_paired_turn = [
        Card::new(Rank::Six, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Clubs),
        Card::new(Rank::Seven, Suit::Spades),
        Card::new(Rank::Jack, Suit::Hearts),
    ];
    let b_7k = postflop_equity_bucket(&hole_7k, &board_paired_turn);
    let (act_7k, tr_7k) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_7k,
            round: 3,
            to_call: 1906,
            pot: 4578,
            hole: Some(&hole_7k),
            board: Some(&board_paired_turn),
            table_bb: 300,
            my_chips: 3285,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_7k, FOLD,
        "Turn middle pair on paired J-high board facing committing barrel must fold (reasons: {:?})",
        tr_7k.guard_reasons
    );

    /* Turn paired + 3-flush board underpair without flush redraw facing >= 3.5 BB donk (25% odds) must fold */
    let hole_44 = [
        Card::new(Rank::Four, Suit::Spades),
        Card::new(Rank::Four, Suit::Clubs),
    ];
    let board_paired_three_flush_turn = [
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Five, Suit::Hearts),
    ];
    let b_44 = postflop_equity_bucket(&hole_44, &board_paired_three_flush_turn);
    let (act_44, tr_44) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_44,
            round: 3,
            to_call: 1500,
            pot: 4500,
            hole: Some(&hole_44),
            board: Some(&board_paired_three_flush_turn),
            table_bb: 400,
            my_chips: 7164,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_44, FOLD,
        "Underpair without flush suit on paired 3-flush turn must fold to 3.75 BB bet (reasons: {:?})",
        tr_44.guard_reasons
    );
}

#[test]
fn test_subgame_turn_bottom_pair_overbet_and_gutshot_folds() {
    let tracker = OpponentTracker::new();
    let overbet_villain = make_overbet_bluffer();
    let mut rng = SmallRng::seed_from_u64(20_261_120);
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];

    let hole_72o = [
        Card::new(Rank::Seven, Suit::Spades),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let board_turn_bottom_pair = [
        Card::new(Rank::Jack, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Clubs),
        Card::new(Rank::King, Suit::Hearts),
    ];
    let b_72o = postflop_equity_bucket(&hole_72o, &board_turn_bottom_pair);
    let (act_72o, _) = overbet_villain.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_72o,
            round: 3,
            to_call: 3600,
            pot: 6000,
            hole: Some(&hole_72o),
            board: Some(&board_turn_bottom_pair),
            table_bb: 200,
            my_chips: 5000,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_72o, FOLD,
        "7s 2c bottom pair must fold to 18 BB turn raise even vs overbet bluffer"
    );

    let hole_67o = [
        Card::new(Rank::Six, Suit::Diamonds),
        Card::new(Rank::Seven, Suit::Hearts),
    ];
    let board_turn_gutshot = [
        Card::new(Rank::Three, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Four, Suit::Clubs),
        Card::new(Rank::Jack, Suit::Spades),
    ];
    let b_67o = postflop_equity_bucket(&hole_67o, &board_turn_gutshot);
    let (act_67o, _) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_67o,
            round: 3,
            to_call: 1247,
            pot: 3157,
            hole: Some(&hole_67o),
            board: Some(&board_turn_gutshot),
            table_bb: 100,
            my_chips: 9000,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_67o, FOLD,
        "6d 7h gutshot must fold on turn facing 40% pot bet in subgame"
    );
}

#[test]
fn test_subgame_low_spr_two_pair_value_and_river_middle_pair_cap() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_120);

    let hole_96o = [
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Six, Suit::Clubs),
    ];
    let board_low_spr_two_pair = [
        Card::new(Rank::Six, Suit::Spades),
        Card::new(Rank::Nine, Suit::Diamonds),
        Card::new(Rank::Five, Suit::Hearts),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let b_96o = postflop_equity_bucket(&hole_96o, &board_low_spr_two_pair);
    let passive_check: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.88, 0.0, 0.12, 0.0, 0.0];
    let mut low_spr_two_pair_bets = 0usize;
    for _ in 0..80 {
        let (act, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &passive_check,
                legal_actions: &LEGAL_OPEN,
                bucket: b_96o,
                round: 3,
                pot: 2152,
                hole: Some(&hole_96o),
                board: Some(&board_low_spr_two_pair),
                table_bb: 200,
                my_chips: 15000,
                opp_chips: Some(1894),
                from_subgame: true,
                ..Default::default()
            },
            &mut rng,
        );
        if act >= RAISE_MIN {
            low_spr_two_pair_bets += 1;
        }
    }
    assert!(
        low_spr_two_pair_bets >= 45,
        "Top Two Pair at effective SPR 0.88 must value-bet majority of the time, got {low_spr_two_pair_bets}/80"
    );

    let hole_t9o = [
        Card::new(Rank::Ten, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Hearts),
    ];
    let board_unpaired_river = [
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Clubs),
        Card::new(Rank::Two, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Hearts),
        Card::new(Rank::King, Suit::Spades),
    ];
    let b_t9o = postflop_equity_bucket(&hole_t9o, &board_unpaired_river);
    let overbet_raw: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.10, 0.0, 0.0, 0.90, 0.0];
    for _ in 0..40 {
        let (act_t9o, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &overbet_raw,
                legal_actions: &LEGAL_OPEN,
                bucket: b_t9o,
                round: 4,
                pot: 1320,
                hole: Some(&hole_t9o),
                board: Some(&board_unpaired_river),
                table_bb: 150,
                my_chips: 14000,
                from_subgame: true,
                ..Default::default()
            },
            &mut rng,
        );
        assert!(
            act_t9o != RAISE_HALF_POT && act_t9o != ALL_IN,
            "Td 9h middle pair must never 125% overbet river, got {act_t9o}"
        );
    }
}

#[test]
fn test_subgame_turn_bottom_pair_shove_vs_maniac() {
    let maniac = make_aggro_maniac();
    let mut rng = SmallRng::seed_from_u64(20_261_121);
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];

    let hole_56s = [
        Card::new(Rank::Five, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Diamonds),
    ];
    let board_turn_bottom_pair = [
        Card::new(Rank::Ace, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Clubs),
        Card::new(Rank::Eight, Suit::Clubs),
        Card::new(Rank::King, Suit::Hearts),
    ];
    let b_56s = postflop_equity_bucket(&hole_56s, &board_turn_bottom_pair);
    let (act_56s, tr_56s) = maniac.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_56s,
            round: 3,
            to_call: 8233,
            pot: 22393,
            hole: Some(&hole_56s),
            board: Some(&board_turn_bottom_pair),
            table_bb: 300,
            my_chips: 8233,
            opp_chips: Some(0),
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_56s, FOLD,
        "Bottom pair must fold to turn all-in at 26.9% pot odds (reasons: {:?})",
        tr_56s.guard_reasons
    );
}

#[test]
fn test_subgame_paired_ahigh_underpair_turn_and_river_folds() {
    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_121);
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];
    let hole_jj = [
        Card::new(Rank::Jack, Suit::Spades),
        Card::new(Rank::Jack, Suit::Hearts),
    ];
    let board_paired_turn = [
        Card::new(Rank::Six, Suit::Hearts),
        Card::new(Rank::Three, Suit::Clubs),
        Card::new(Rank::Six, Suit::Spades),
        Card::new(Rank::Ace, Suit::Hearts),
    ];
    let b_jj_turn = postflop_equity_bucket(&hole_jj, &board_paired_turn);
    let (act_jj_turn, tr_jj_turn) = neutral.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_jj_turn,
            round: 3,
            to_call: 3375,
            pot: 7125,
            hole: Some(&hole_jj),
            board: Some(&board_paired_turn),
            table_bb: 150,
            my_chips: 7975,
            opp_chips: Some(4600),
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_jj_turn, FOLD,
        "Underpair on paired A-high turn must fold to 90% pot barrel (reasons: {:?})",
        tr_jj_turn.guard_reasons
    );

    let board_paired_river = [
        Card::new(Rank::Six, Suit::Hearts),
        Card::new(Rank::Three, Suit::Clubs),
        Card::new(Rank::Six, Suit::Spades),
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Five, Suit::Diamonds),
    ];
    let b_jj_river = postflop_equity_bucket(&hole_jj, &board_paired_river);
    let (act_jj_river, tr_jj_river) = neutral.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_jj_river,
            round: 4,
            to_call: 4600,
            pot: 15100,
            hole: Some(&hole_jj),
            board: Some(&board_paired_river),
            table_bb: 150,
            my_chips: 4900,
            opp_chips: Some(0),
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_jj_river, FOLD,
        "Underpair on paired A-high river must fold to explicit opp all-in (reasons: {:?})",
        tr_jj_river.guard_reasons
    );
}

#[test]
fn test_subgame_turn_underpair_reraise_cap() {
    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_121);
    let shove_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.10, 0.0, 0.0, 0.25, 0.60];

    let hole_66 = [
        Card::new(Rank::Six, Suit::Hearts),
        Card::new(Rank::Six, Suit::Clubs),
    ];
    let board_turn_kc = [
        Card::new(Rank::Two, Suit::Clubs),
        Card::new(Rank::Three, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Diamonds),
        Card::new(Rank::King, Suit::Clubs),
    ];
    let b_66 = postflop_equity_bucket(&hole_66, &board_turn_kc);
    for _ in 0..40 {
        let (act_66, _) = neutral.select_action_traced(
            &PolicyRequest {
                raw_probs: &shove_heavy,
                legal_actions: &LEGAL_FULL,
                bucket: b_66,
                round: 3,
                to_call: 1200,
                pot: 3600,
                hole: Some(&hole_66),
                board: Some(&board_turn_kc),
                table_bb: 400,
                my_chips: 7218,
                opp_chips: Some(8000),
                from_subgame: true,
                ..Default::default()
            },
            &mut rng,
        );
        assert!(
            act_66 < RAISE_MIN,
            "Turn underpair facing 3 BB bet must never raise or shove (got {act_66})"
        );
    }
}

#[test]
fn test_subgame_baby_vs_qhigh_four_flush_river() {
    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_121);
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];
    let pure_call: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0];

    let board_four_flush = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Six, Suit::Hearts),
        Card::new(Rank::Ten, Suit::Diamonds),
        Card::new(Rank::Eight, Suit::Hearts),
    ];
    let hole_54h = [
        Card::new(Rank::Five, Suit::Hearts),
        Card::new(Rank::Four, Suit::Hearts),
    ];
    let b_54h = postflop_equity_bucket(&hole_54h, &board_four_flush);
    assert_eq!(b_54h, 79, "5-high flush on 4-flush board maps to bucket 79");
    let (act_54h, tr_54h) = neutral.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_54h,
            round: 4,
            to_call: 1498,
            pot: 3498,
            hole: Some(&hole_54h),
            board: Some(&board_four_flush),
            table_bb: 300,
            my_chips: 9000,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_54h, FOLD,
        "Baby flush (bucket 79) on 4-flush river must fold to 75% pot bet (reasons: {:?})",
        tr_54h.guard_reasons
    );

    let hole_qh = [
        Card::new(Rank::Queen, Suit::Hearts),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let b_qh = postflop_equity_bucket(&hole_qh, &board_four_flush);
    assert!(b_qh >= 80, "Q-high flush must stay in bucket >= 80");
    let (act_qh, tr_qh) = neutral.select_action_traced(
        &PolicyRequest {
            raw_probs: &pure_call,
            legal_actions: &LEGAL_FULL,
            bucket: b_qh,
            round: 4,
            to_call: 1498,
            pot: 3498,
            hole: Some(&hole_qh),
            board: Some(&board_four_flush),
            table_bb: 300,
            my_chips: 9000,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert!(
        !tr_qh.guard_reasons.contains(&"SHOVE_DEFENSE_FOLD"),
        "Q-high flush must not trigger SHOVE_DEFENSE_FOLD"
    );
    assert_ne!(act_qh, FOLD, "Q-high flush on 4-flush river must not fold");
}

#[test]
fn test_subgame_turn_pot_control_three_flush_and_four_straight_folds() {
    let neutral = OpponentTracker::new();
    let overbet_villain = make_overbet_bluffer();
    let mut rng = SmallRng::seed_from_u64(20_261_122);
    let bet_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.10, 0.10, 0.40, 0.40, 0.0];
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];

    /* 1. Middle/weak pair facing no bet on bloated Turn must check for pot control */
    let hole_t9 = [
        Card::new(Rank::Ten, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Diamonds),
    ];
    let board_t9_turn = [
        Card::new(Rank::Three, Suit::Hearts),
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Jack, Suit::Clubs),
    ];
    let b_t9 = postflop_equity_bucket(&hole_t9, &board_t9_turn);
    for _ in 0..40 {
        let (act, _) = neutral.select_action_traced(
            &PolicyRequest {
                raw_probs: &bet_heavy,
                legal_actions: &LEGAL_OPEN,
                bucket: b_t9,
                round: 3,
                pot: 3000,
                hole: Some(&hole_t9),
                board: Some(&board_t9_turn),
                table_bb: 200,
                my_chips: 12000,
                from_subgame: true,
                ..Default::default()
            },
            &mut rng,
        );
        assert_eq!(
            act, CALL_CHECK,
            "Turn middle/weak pair in bloated pot must check instead of leading out (got {act})"
        );
    }

    /* 2. Second pair without flush redraw on 3-flush Turn must fold to all-in even vs overbet bluffer */
    let hole_j2 = [
        Card::new(Rank::Jack, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let board_three_flush_turn = [
        Card::new(Rank::Three, Suit::Diamonds),
        Card::new(Rank::Jack, Suit::Spades),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::King, Suit::Diamonds),
    ];
    let b_j2 = postflop_equity_bucket(&hole_j2, &board_three_flush_turn);
    let (act_j2, tr_j2) = overbet_villain.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_j2,
            round: 3,
            to_call: 4040,
            pot: 7160,
            hole: Some(&hole_j2),
            board: Some(&board_three_flush_turn),
            table_bb: 400,
            my_chips: 4040,
            opp_chips: Some(0),
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_j2, FOLD,
        "Below-top-pair on 3-flush turn must fold to overbet jam even vs overbet bluffer (reasons: {:?})",
        tr_j2.guard_reasons
    );

    /* 3. Bare top pair on 4-to-a-straight Turn facing heavy barrel must fold */
    let hole_k7 = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Seven, Suit::Diamonds),
    ];
    let board_four_straight_turn = [
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Ten, Suit::Diamonds),
    ];
    let b_k7 = postflop_equity_bucket(&hole_k7, &board_four_straight_turn);
    let (act_k7, tr_k7) = neutral.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_k7,
            round: 3,
            to_call: 2812,
            pot: 6262,
            hole: Some(&hole_k7),
            board: Some(&board_four_straight_turn),
            table_bb: 300,
            my_chips: 5649,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_k7, FOLD,
        "Bare top pair on 4-straight turn must fold to 80% pot barrel (reasons: {:?})",
        tr_k7.guard_reasons
    );

    /* 4. Two pair facing 75% pot bet on 3-flush river in subgame must flat-call instead of raising into flush */
    let hole_qcts = [
        Card::new(Rank::Queen, Suit::Clubs),
        Card::new(Rank::Ten, Suit::Spades),
    ];
    let board_three_flush_river = [
        Card::new(Rank::Six, Suit::Spades),
        Card::new(Rank::Four, Suit::Spades),
        Card::new(Rank::Ten, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Clubs),
        Card::new(Rank::Queen, Suit::Spades),
    ];
    let b_qcts = postflop_equity_bucket(&hole_qcts, &board_three_flush_river);
    let raise_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.20, 0.25, 0.30, 0.0, 0.20];
    for _ in 0..40 {
        let (act, _) = neutral.select_action_traced(
            &PolicyRequest {
                raw_probs: &raise_heavy,
                legal_actions: &LEGAL_FULL,
                bucket: b_qcts,
                round: 4,
                to_call: 2478,
                pot: 5782,
                hole: Some(&hole_qcts),
                board: Some(&board_three_flush_river),
                table_bb: 150,
                my_chips: 8300,
                from_subgame: true,
                ..Default::default()
            },
            &mut rng,
        );
        assert!(
            act < RAISE_MIN,
            "Two pair without flush facing 75% bet on 3-flush river must never raise (got {act})"
        );
    }

    /* 5. Underpair to King in 3-bet pot facing >=15 BB turn barrel committing >=50% stack must fold */
    let hole_qq = [
        Card::new(Rank::Queen, Suit::Clubs),
        Card::new(Rank::Queen, Suit::Hearts),
    ];
    let board_k_high_turn = [
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Two, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Spades),
    ];
    let b_qq = postflop_equity_bucket(&hole_qq, &board_k_high_turn);
    let (act_qq, tr_qq) = neutral.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_qq,
            round: 3,
            to_call: 3666,
            pot: 10578,
            hole: Some(&hole_qq),
            board: Some(&board_k_high_turn),
            table_bb: 200,
            my_chips: 7026,
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_qq, FOLD,
        "Underpair QQ on K-high turn facing 18 BB barrel committing >50% stack must fold (reasons: {:?})",
        tr_qq.guard_reasons
    );
}

#[test]
fn test_subgame_early_overbet_river_thin_value_and_turn_two_pair_cap() {
    let mut rng = SmallRng::seed_from_u64(20_261_123);
    let mut early_overbet = OpponentTracker::new();
    /* 2 overbets out of first 2 postflop bets must already mark is_overbet_bluffer() */
    early_overbet.record_action_street_detailed(RAISE_HALF_POT, 3, false, 2400, 1600);
    early_overbet.record_action_street_detailed(RAISE_HALF_POT, 4, false, 9184, 2666);
    assert!(early_overbet.is_overbet_bluffer());

    /* 1. Top pair Qc Js on paired Q-high river vs early 3.4x overbet-jam must not force-fold */
    let hole_qj = [
        Card::new(Rank::Queen, Suit::Clubs),
        Card::new(Rank::Jack, Suit::Spades),
    ];
    let board_paired_q = [
        Card::new(Rank::Eight, Suit::Hearts),
        Card::new(Rank::Five, Suit::Diamonds),
        Card::new(Rank::Ten, Suit::Diamonds),
        Card::new(Rank::Eight, Suit::Spades),
        Card::new(Rank::Queen, Suit::Spades),
    ];
    let b_qj = postflop_equity_bucket(&hole_qj, &board_paired_q);
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];
    let (act_qj, tr_qj) = early_overbet.select_action_traced(
        &PolicyRequest {
            raw_probs: &call_heavy,
            legal_actions: &LEGAL_FULL,
            bucket: b_qj,
            round: 4,
            to_call: 9184,
            pot: 11850,
            hole: Some(&hole_qj),
            board: Some(&board_paired_q),
            table_bb: 100,
            my_chips: 9184,
            opp_chips: Some(0),
            from_subgame: true,
            ..Default::default()
        },
        &mut rng,
    );
    assert!(
        act_qj != FOLD && !tr_qj.guard_reasons.contains(&"SHOVE_DEFENSE_FOLD"),
        "Top pair Qc Js vs early overbet spammer must not force-fold (reasons: {:?})",
        tr_qj.guard_reasons
    );

    /* 2. Clean unpaired River middle pair (bucket 36) fires 33% thin value instead of 95% CHECK cap */
    let neutral = OpponentTracker::new();
    let hole_qt = [
        Card::new(Rank::Queen, Suit::Hearts),
        Card::new(Rank::Ten, Suit::Clubs),
    ];
    let board_clean_river = [
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Ten, Suit::Spades),
        Card::new(Rank::Six, Suit::Clubs),
        Card::new(Rank::Three, Suit::Hearts),
        Card::new(Rank::Two, Suit::Spades),
    ];
    let b_qt = postflop_equity_bucket(&hole_qt, &board_clean_river);
    let mut thin_bets = 0usize;
    for _ in 0..80 {
        let (act, _) = neutral.select_action_traced(
            &PolicyRequest {
                raw_probs: &[0.0, 0.70, 0.25, 0.05, 0.0, 0.0],
                legal_actions: &LEGAL_OPEN,
                bucket: b_qt,
                round: 4,
                pot: 800,
                hole: Some(&hole_qt),
                board: Some(&board_clean_river),
                table_bb: 100,
                my_chips: 8000,
                from_subgame: true,
                ..Default::default()
            },
            &mut rng,
        );
        if act == RAISE_MIN {
            thin_bets += 1;
        }
    }
    assert!(
        thin_bets >= 30,
        "Clean unpaired River middle pair must thin-value bet frequently (got {thin_bets}/80)"
    );

    /* 3. Deep-stack (>55 BB) Turn bottom two pair (bucket 58) facing 15 BB raise must flat-call, not 3-bet */
    let hole_34 = [
        Card::new(Rank::Three, Suit::Clubs),
        Card::new(Rank::Four, Suit::Clubs),
    ];
    let board_aj43 = [
        Card::new(Rank::Four, Suit::Spades),
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Clubs),
        Card::new(Rank::Three, Suit::Hearts),
    ];
    let b_34 = postflop_equity_bucket(&hole_34, &board_aj43);
    assert_eq!(b_34, 58);
    let raise_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.20, 0.30, 0.30, 0.20, 0.0];
    for _ in 0..40 {
        let (act_34, _) = neutral.select_action_traced(
            &PolicyRequest {
                raw_probs: &raise_heavy,
                legal_actions: &LEGAL_FULL,
                bucket: b_34,
                round: 3,
                to_call: 1574,
                pot: 3411,
                hole: Some(&hole_34),
                board: Some(&board_aj43),
                table_bb: 100,
                my_chips: 9000,
                from_subgame: true,
                ..Default::default()
            },
            &mut rng,
        );
        assert_eq!(
            act_34, CALL_CHECK,
            "Deep-stack Turn bottom two pair facing raise must flat-call instead of 3-betting (got {act_34})"
        );
    }

    /* 4. Low-aggression polarized bettor with sporadic 150% river value bets must NOT trigger is_overbet_bluffer() */
    let mut balanced_polarized = OpponentTracker::new();
    for _ in 0..8 {
        balanced_polarized.record_action_street_detailed(CALL_CHECK, 1, true, 0, 300);
        balanced_polarized.record_action_street_detailed(CALL_CHECK, 2, false, 0, 500);
        balanced_polarized.record_action_street_detailed(CALL_CHECK, 3, false, 0, 500);
        balanced_polarized.end_hand();
    }
    for _ in 0..4 {
        balanced_polarized.record_action_street_detailed(RAISE_THIRD_POT, 2, false, 300, 500);
        balanced_polarized.record_action_street_detailed(RAISE_HALF_POT, 4, false, 1650, 1100);
        balanced_polarized.end_hand();
    }
    assert!(
        !balanced_polarized.is_overbet_bluffer(),
        "Low-aggression opponent (raise_ratio < 0.30) with sporadic river overbets must not be flagged"
    );
    assert!(
        balanced_polarized
            .subgame_bluff_multiplier()
            .is_none_or(|m| m < 1.0),
        "Balanced polarized bettor must not receive inflated subgame bluff multiplier"
    );

    /* 5. Value-heavy trapper showing monsters at showdown vetoes is_overbet_bluffer() */
    let mut value_trapper = OpponentTracker::new();
    let hole_set = [
        Card::new(Rank::Seven, Suit::Spades),
        Card::new(Rank::Seven, Suit::Hearts),
    ];
    let board_set = [
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Four, Suit::Clubs),
    ];
    for _ in 0..4 {
        value_trapper.record_action_street_detailed(RAISE_HALF_POT, 4, false, 1500, 1000);
        value_trapper.record_showdown_hand_detailed(hole_set, &board_set, true, 1500, 1000);
        value_trapper.end_hand();
    }
    assert!(
        !value_trapper.is_overbet_bluffer(),
        "Showdown-confirmed value monster overbettor must be vetoed from is_overbet_bluffer()"
    );
}
