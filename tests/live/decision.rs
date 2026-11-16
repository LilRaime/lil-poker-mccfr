use lil_poker_mccfr::cfr::abstraction::{nash_open_push_max_bb, preflop_bucket};
use lil_poker_mccfr::cfr::fallback::StrategyLookupIndex;
use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
use lil_poker_mccfr::cfr::subgame::{SubgameContext, SubgameSolver};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{
    Card, Rank, RoundHistory, Suit, ALL_IN, CALL_CHECK, FOLD, NUM_ROUNDS, RAISE_HALF_POT,
    RAISE_MIN, RAISE_THIRD_POT,
};
use lil_poker_mccfr::live::decision::{
    decide_action, decide_action_multiway_context, decide_action_traced_with_bet,
};
use lil_poker_mccfr::live::protocol::map_action_index_full;
use lil_poker_mccfr::live::state::derive_legal_actions;
use std::collections::HashMap;

#[test]
fn test_live_decide_action_with_real_contributions() {
    let hole = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Spades),
    ];
    let board = [
        Card::new(Rank::Queen, Suit::Spades),
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Ten, Suit::Diamonds),
    ];
    let legal = vec![
        "fold".to_string(),
        "check".to_string(),
        "bet".to_string(),
        "allin".to_string(),
    ];
    let tracker = OpponentTracker::new();
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let index = StrategyLookupIndex::new();
    let solver = SubgameSolver::new(50);
    let mut ctx = SubgameContext::new();

    /* 1. As player 0 (SB) with contributions [100, 100], to_call = 0 */
    let (act1, amt1) = decide_action(
        &hole,
        &board,
        &legal,
        0,
        200,
        &strategy_map,
        true, /* subgame search on */
        &tracker,
        &history,
        0,
        [100, 100],
        0,
        Some(&index),
        Some(&solver),
        Some(&mut ctx),
    );
    assert!(!act1.is_empty());
    assert!(amt1 >= 0);

    /* 2. As player 1 (BB) facing a raise with contributions [250, 100], to_call = 150 */
    let legal_facing_raise = vec![
        "fold".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];
    let (act2, amt2) = decide_action(
        &hole,
        &board,
        &legal_facing_raise,
        150,
        350,
        &strategy_map,
        true,
        &tracker,
        &history,
        1,
        [250, 100],
        1,
        Some(&index),
        Some(&solver),
        Some(&mut ctx),
    );
    assert!(!act2.is_empty());
    assert!(amt2 >= 0);
}

#[test]
fn test_bot_value_bets_straight_in_live_game() {
    /* Bot holds [10s, 8d], Board on Turn is [Qd, 10c, Js, 9d] -> Straight, Queen High */
    let hole = [
        Card::new(Rank::Ten, Suit::Spades),
        Card::new(Rank::Eight, Suit::Diamonds),
    ];
    let board = [
        Card::new(Rank::Queen, Suit::Diamonds),
        Card::new(Rank::Ten, Suit::Clubs),
        Card::new(Rank::Jack, Suit::Spades),
        Card::new(Rank::Nine, Suit::Diamonds),
    ];

    let legal = derive_legal_actions(0, 9600);
    let tracker = OpponentTracker::new();
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let solver = SubgameSolver::new(400);
    let mut ctx = SubgameContext::new();
    let empty_map = HashMap::new();
    let index = StrategyLookupIndex::new();

    let mut raise_count = 0;
    let num_trials = 60;

    for _ in 0..num_trials {
        let (action, amount) = decide_action(
            &hole,
            &board,
            &legal,
            0,
            200,
            &empty_map,
            true,
            &tracker,
            &history,
            1,
            [100, 100],
            0,
            Some(&index),
            Some(&solver),
            Some(&mut ctx),
        );

        if action == "raise" {
            raise_count += 1;
            assert!(amount >= 100, "Raise amount must be at least min_raise");
        } else if action == "all_in" || action == "allin" {
            raise_count += 1;
        }
    }

    let raise_rate = raise_count as f64 / num_trials as f64;
    assert!(
        raise_rate >= 0.50,
        "Expected bot with Straight to value bet at least 50% of the time, got {:.1}% ({}/{})",
        raise_rate * 100.0,
        raise_count,
        num_trials
    );
}

#[test]
fn test_short_stack_bb_check_and_pot_commitment() {
    let tracker = OpponentTracker::new();
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let legal_bb = vec![
        "fold".to_string(),
        "check".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];

    /* 1. Short stack (11 BB) in BB facing a limp with a weak Tier 6 hand (9h 6h) must check */
    let hole_96s = [
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Six, Suit::Hearts),
    ];
    let (act_bb_limp, _) = decide_action_multiway_context(
        &hole_96s,
        &[],
        &legal_bb,
        0,
        400,
        &strategy_map,
        true,
        &tracker,
        &history,
        1,
        [200, 200],
        0,
        None,
        None,
        None,
        1,
        200,
        2200,
    );
    assert_eq!(
        act_bb_limp, "check",
        "Weak suited connector in BB facing a limp at 11 BB must check, not shove!"
    );

    /* 2. Postflop Turn with 2 chips remaining into a huge pot must call, NEVER fold */
    let hole_k7o = [
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Seven, Suit::Clubs),
    ];
    let board_turn = vec![
        Card::new(Rank::Three, Suit::Hearts),
        Card::new(Rank::Four, Suit::Diamonds),
        Card::new(Rank::Four, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Spades),
    ];
    let legal_call = vec!["fold".to_string(), "call".to_string(), "allin".to_string()];
    let (act_micro_call, _) = decide_action_multiway_context(
        &hole_k7o,
        &board_turn,
        &legal_call,
        2,
        10940,
        &strategy_map,
        true,
        &tracker,
        &history,
        0,
        [5469, 5471],
        2,
        None,
        None,
        None,
        1,
        200,
        2,
    );
    assert_ne!(
        act_micro_call, "fold",
        "Facing a 2-chip bet into a 10,940 pot must never fold!"
    );
}

#[test]
fn test_6max_small_pair_folds_8x_open_and_flop_shove() {
    let tracker = OpponentTracker::new();
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let hole_66 = [
        Card::new(Rank::Six, Suit::Hearts),
        Card::new(Rank::Six, Suit::Diamonds),
    ];
    let legal = vec![
        "fold".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];

    /* 1. Preflop 6-max (200/400): 66 facing 8x open (to_call = 3200, pot = 3800) must fold */
    let (pf_act, _, _) = decide_action_traced_with_bet(
        &hole_66,
        &[],
        &legal,
        3200,
        3800,
        &strategy_map,
        false,
        &tracker,
        &history,
        1,
        [200, 3200],
        1,
        None,
        None,
        None,
        2,
        400,
        12000,
        3200,
    );
    assert_eq!(
        pf_act, "fold",
        "66 must fold preflop facing an 8x open raise"
    );

    /* 2. Flop J-8-2 facing all-in shove where to_call == my_chips (66 < second pair) must fold */
    let board_j82 = vec![
        Card::new(Rank::Jack, Suit::Clubs),
        Card::new(Rank::Eight, Suit::Spades),
        Card::new(Rank::Two, Suit::Diamonds),
    ];
    let legal_shove = vec!["fold".to_string(), "call".to_string(), "allin".to_string()];
    let (flop_act, _, _) = decide_action_traced_with_bet(
        &hole_66,
        &board_j82,
        &legal_shove,
        8000,
        15200,
        &strategy_map,
        false,
        &tracker,
        &history,
        1,
        [3200, 3200],
        1,
        None,
        None,
        None,
        1,
        400,
        8000,
        8000,
    );
    assert_eq!(
        flop_act, "fold",
        "66 on J-8-2 facing an all-in call (to_call == my_chips) must fold"
    );
}

#[test]
fn test_hu_bare_flush_draw_flop_raise_and_3bet_discipline() {
    let tracker = OpponentTracker::new();
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let hole_j6s = [
        Card::new(Rank::Jack, Suit::Spades),
        Card::new(Rank::Six, Suit::Spades),
    ];
    let board_583 = vec![
        Card::new(Rank::Five, Suit::Spades),
        Card::new(Rank::Eight, Suit::Clubs),
        Card::new(Rank::Three, Suit::Spades),
    ];
    let legal = vec![
        "fold".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];

    /* 1. Facing 4x BB bet (600 into 1200) on flop with bare flush draw (Js6s): must not check-raise */
    let (act_vs_bet, _, _) = decide_action_traced_with_bet(
        &hole_j6s,
        &board_583,
        &legal,
        600,
        1200,
        &strategy_map,
        false,
        &tracker,
        &history,
        1,
        [300, 300],
        0,
        None,
        None,
        None,
        1,
        150,
        9000,
        600,
    );
    assert_ne!(
        act_vs_bet, "raise",
        "Bare draw should not check-raise vs large flop bet"
    );
    assert_ne!(
        act_vs_bet, "allin",
        "Bare draw should not shove vs flop bet"
    );

    /* 2. Facing 25 BB 3-bet on flop (3750 into 7500, 33.3% pot odds) with bare flush draw: must fold */
    let (act_vs_3bet, _, _) = decide_action_traced_with_bet(
        &hole_j6s,
        &board_583,
        &legal,
        3750,
        7500,
        &strategy_map,
        false,
        &tracker,
        &history,
        1,
        [300, 300],
        1,
        None,
        None,
        None,
        1,
        150,
        8000,
        3750,
    );
    assert_eq!(
        act_vs_3bet, "fold",
        "Bare flush draw must fold facing 25 BB flop 3-bet at ~33% pot odds"
    );
}

#[test]
fn test_hu_suited_connector_12bb_no_open_jam() {
    let hole_76s = [
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Six, Suit::Hearts),
    ];
    let (idx_76s, _) = preflop_bucket(hole_76s[0], hole_76s[1]);
    assert!(
        nash_open_push_max_bb(idx_76s, 1) <= 9,
        "76s push threshold should be <= 9 BB (got {})",
        nash_open_push_max_bb(idx_76s, 1)
    );

    let tracker = OpponentTracker::new();
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let legal = vec![
        "fold".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];
    let (act, _, _) = decide_action_traced_with_bet(
        &hole_76s,
        &[],
        &legal,
        400,
        1200,
        &strategy_map,
        false,
        &tracker,
        &history,
        0,
        [400, 800],
        0,
        None,
        None,
        None,
        1,
        800,
        10000,
        800,
    );
    assert_ne!(
        act, "allin",
        "76s at 12.5 BB (10,000 chips @ 400/800) must not open-jam all-in"
    );
}

#[test]
fn test_small_pocket_pair_no_bloated_3bet_and_protocol_sizing() {
    let tracker = OpponentTracker::new();
    let hole_55 = [
        Card::new(Rank::Five, Suit::Hearts),
        Card::new(Rank::Five, Suit::Spades),
    ];
    let (bucket_55, _) = preflop_bucket(hole_55[0], hole_55[1]);
    let mut rng = rand::thread_rng();
    let policy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.25, 0.20, 0.25, 0.30, 0.0];
    let legal_idx = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];

    let mut raise_min_count = 0usize;
    let mut trace_has_guard = false;
    for _ in 0..100 {
        let (act, trace) = tracker.select_action_purified_traced_with_stack(
            &policy,
            &legal_idx,
            bucket_55,
            1,
            350,
            1100,
            Some(&hole_55),
            None,
            1,
            300,
            15000,
            &mut rng,
        );
        assert!(
            act != RAISE_THIRD_POT && act != RAISE_HALF_POT && act != ALL_IN,
            "55 facing open raise must suppress 3-bet raise sizes, got action {}",
            act
        );
        if act == RAISE_MIN {
            raise_min_count += 1;
        }
        if trace.guard_reasons.contains(&"PF_DISCIPLINE_GUARD") {
            trace_has_guard = true;
        }
    }
    assert!(
        trace_has_guard,
        "PF_DISCIPLINE_GUARD must fire for 55 facing open raise"
    );
    assert!(
        raise_min_count <= 20,
        "55 should rarely 3-bet min-raise (capped <= 0.05), got {}/100",
        raise_min_count
    );

    let legal = vec![
        "fold".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];
    let (_, three_bet_amt) = map_action_index_full(
        RAISE_HALF_POT as usize,
        &legal,
        350,
        1100,
        false,
        1,
        300,
        15000,
        650,
    )
    .expect("map 3-bet");
    assert!(
        three_bet_amt <= 1950,
        "Preflop RAISE_HALF_POT 3-bet vs 650 open must be <= 3.0x (1950), got {}",
        three_bet_amt
    );
}

#[test]
fn test_33bb_aqo_folds_vs_tag_shove_and_calls_maniac() {
    let hole_aqo = [
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::Queen, Suit::Diamonds),
    ];
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let legal = vec!["fold".to_string(), "call".to_string(), "allin".to_string()];

    /* 1. TAG opponent (~31% VPIP, ~25% PFR): AQo folds to a 33 BB shove (33 > 30 BB HU Nash cap) */
    let mut tag_tracker = OpponentTracker::new();
    for i in 0..16 {
        if i < 4 {
            tag_tracker.record_action_street_detailed(RAISE_MIN, 1, false, 400, 600);
            tag_tracker.record_preflop_positional(true, true, true);
        } else if i == 4 {
            tag_tracker.record_action_street_detailed(CALL_CHECK, 1, true, 200, 600);
            tag_tracker.record_preflop_positional(false, true, false);
        } else {
            tag_tracker.record_action_street_detailed(FOLD, 1, true, 0, 600);
            tag_tracker.record_preflop_positional(false, false, false);
        }
        tag_tracker.end_hand();
    }
    let (tag_act, _, _) = decide_action_traced_with_bet(
        &hole_aqo,
        &[],
        &legal,
        6600,
        7000,
        &strategy_map,
        false,
        &tag_tracker,
        &history,
        1,
        [200, 200],
        1,
        None,
        None,
        None,
        1,
        200,
        6600,
        6600,
    );
    assert_eq!(tag_act, "fold", "AQo must fold vs non-maniac 33 BB shove");

    /* 2. TAG opponent at 28 BB: within HU Nash call cap (28 <= 30 BB), mixed/majority call */
    let mut hu_28_calls = 0usize;
    for _ in 0..30 {
        let (hu_28_act, _, _) = decide_action_traced_with_bet(
            &hole_aqo,
            &[],
            &legal,
            5600,
            6000,
            &strategy_map,
            false,
            &tag_tracker,
            &history,
            1,
            [200, 200],
            1,
            None,
            None,
            None,
            1,
            200,
            5600,
            5600,
        );
        if hu_28_act != "fold" {
            hu_28_calls += 1;
        }
    }
    assert!(
        hu_28_calls >= 12,
        "AQo should frequently call <= 30 BB HU shove (got {}/30 calls)",
        hu_28_calls
    );

    /* 3. Maniac opponent at 33 BB: expanded maniac cap (33 <= 38 BB) calls */
    let mut maniac_tracker = OpponentTracker::new();
    for _ in 0..16 {
        maniac_tracker.record_action_street_detailed(ALL_IN, 1, false, 4000, 600);
        maniac_tracker.record_preflop_positional(true, true, true);
        maniac_tracker.end_hand();
    }
    let (maniac_act, _, _) = decide_action_traced_with_bet(
        &hole_aqo,
        &[],
        &legal,
        6600,
        7000,
        &strategy_map,
        false,
        &maniac_tracker,
        &history,
        1,
        [200, 200],
        1,
        None,
        None,
        None,
        1,
        200,
        6600,
        6600,
    );
    assert_ne!(maniac_act, "fold", "AQo should call maniac 33 BB shove");
}

#[test]
fn test_preflop_micro_stack_1chip_calls_and_deep_99_no_open_jam() {
    let tracker = OpponentTracker::new();
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();

    /* 1. Micro-stack preflop commitment: Kd Qc with 1 chip remaining facing 1-chip effective call into 5,248 pot
     * must return "call" (NEVER "fold" and NEVER "allin"/"raise" when to_call >= my_chips) */
    let hole_kqo = [
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Queen, Suit::Clubs),
    ];
    let legal_1chip = vec!["fold".to_string(), "call".to_string(), "allin".to_string()];
    let (act_1chip, _, tr_1chip) = decide_action_traced_with_bet(
        &hole_kqo,
        &[],
        &legal_1chip,
        1,
        5248,
        &strategy_map,
        false,
        &tracker,
        &history,
        1,
        [2624, 2624],
        1,
        None,
        None,
        None,
        1,
        400,
        1,
        13124,
    );
    assert_eq!(
        act_1chip, "call",
        "With 1 chip remaining facing 1 chip to call in 5248 pot, must return 'call' (reasons: {:?})",
        tr_1chip.guard_reasons
    );

    /* 2. Deep 6-max unopened pot (~30 BB): 9c 9h (8,938 @ 150/300)
     * must open-raise cleanly and never shove ALL_IN */
    let hole_99 = [
        Card::new(Rank::Nine, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Hearts),
    ];
    let legal_full = vec![
        "fold".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "all_in".to_string(),
    ];
    for _ in 0..30 {
        let (act_99, _, _) = decide_action_traced_with_bet(
            &hole_99,
            &[],
            &legal_full,
            300,
            450,
            &strategy_map,
            false,
            &tracker,
            &history,
            0,
            [0, 300],
            0,
            None,
            None,
            None,
            5,
            300,
            8938,
            300,
        );
        assert_ne!(
            act_99, "all_in",
            "99 at 29.8 BB in 6-max unopened pot must never open-jam all-in"
        );
    }
}
