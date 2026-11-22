use lil_poker_mccfr::cfr::abstraction::{nash_open_push_max_bb, preflop_bucket};
use lil_poker_mccfr::cfr::fallback::StrategyLookupIndex;
use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
use lil_poker_mccfr::cfr::subgame::{SubgameContext, SubgameSolver};
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, Card, FOLD, NUM_ROUNDS, RAISE_MIN, Rank, RoundHistory, Suit,
};
use lil_poker_mccfr::live::decision::{LiveDecisionRequest, decide_action, decide_action_traced};
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
        &LiveDecisionRequest {
            hole,
            board: &board,
            legal: &legal,
            pot: 200,
            subgame_search: true,
            history,
            contributions: [100, 100],
            strategy_index: Some(&index),
            solver: Some(&solver),
            ..Default::default()
        },
        &strategy_map,
        &tracker,
        Some(&mut ctx),
    );
    assert!(!act1.is_empty() && act1 != "fold");
    assert!(amt1 >= 0);

    /* 2. As player 1 (BB) facing a raise with contributions [250, 100], to_call = 150 */
    let legal_facing_raise = vec![
        "fold".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];
    let (act2, amt2) = decide_action(
        &LiveDecisionRequest {
            hole,
            board: &board,
            legal: &legal_facing_raise,
            to_call: 150,
            pot: 350,
            subgame_search: true,
            history,
            my_player: 1,
            contributions: [250, 100],
            raises_this_round: 1,
            strategy_index: Some(&index),
            solver: Some(&solver),
            ..Default::default()
        },
        &strategy_map,
        &tracker,
        Some(&mut ctx),
    );
    assert!(!act2.is_empty() && act2 != "fold");
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
            &LiveDecisionRequest {
                hole,
                board: &board,
                legal: &legal,
                pot: 200,
                subgame_search: true,
                history,
                my_player: 1,
                contributions: [100, 100],
                strategy_index: Some(&index),
                solver: Some(&solver),
                ..Default::default()
            },
            &empty_map,
            &tracker,
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
fn test_short_stack_bb_check_and_min_raise_defense() {
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
    let (act_bb_limp, _) = decide_action(
        &LiveDecisionRequest {
            hole: hole_96s,
            legal: &legal_bb,
            pot: 400,
            subgame_search: true,
            history,
            my_player: 1,
            contributions: [200, 200],
            big_blind: 200,
            my_chips: 2200,
            ..Default::default()
        },
        &strategy_map,
        &tracker,
        None,
    );
    assert_eq!(
        act_bb_limp, "check",
        "Weak suited connector in BB facing a limp at 11 BB must check, not shove!"
    );

    /* 2. In BB at ~10 BB (my_chips = 1950, bb = 200) facing a 2x min-raise (to_call = 200, pot = 600)
     * with offsuit Ace-rag (A4o, Tier 4), bot should flat call to see the flop rather than open-jamming all-in! */
    let hole_a4o = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Four, Suit::Clubs),
    ];
    let legal_pf = vec![
        "fold".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];
    let (act_bb_defend, _) = decide_action(
        &LiveDecisionRequest {
            hole: hole_a4o,
            legal: &legal_pf,
            to_call: 200,
            pot: 600,
            subgame_search: true,
            history,
            my_player: 1,
            contributions: [200, 400],
            big_blind: 200,
            my_chips: 1950,
            ..Default::default()
        },
        &strategy_map,
        &tracker,
        None,
    );
    assert_eq!(
        act_bb_defend, "call",
        "In BB at ~10 BB facing a 2x min-raise with A4o, bot should flat-call rather than jam all-in"
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
    let (pf_act, _, _) = decide_action_traced(
        &LiveDecisionRequest {
            hole: hole_66,
            legal: &legal,
            to_call: 3200,
            pot: 3800,
            history,
            my_player: 1,
            contributions: [200, 3200],
            raises_this_round: 1,
            active_opponents: 2,
            big_blind: 400,
            my_chips: 12000,
            current_bet: 3200,
            ..Default::default()
        },
        &strategy_map,
        &tracker,
        None,
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
    let (flop_act, _, _) = decide_action_traced(
        &LiveDecisionRequest {
            hole: hole_66,
            board: &board_j82,
            legal: &legal_shove,
            to_call: 8000,
            pot: 15200,
            history,
            my_player: 1,
            contributions: [3200, 3200],
            raises_this_round: 1,
            big_blind: 400,
            my_chips: 8000,
            current_bet: 8000,
            ..Default::default()
        },
        &strategy_map,
        &tracker,
        None,
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
    let (act_vs_bet, _, _) = decide_action_traced(
        &LiveDecisionRequest {
            hole: hole_j6s,
            board: &board_583,
            legal: &legal,
            to_call: 600,
            pot: 1200,
            history,
            my_player: 1,
            contributions: [300, 300],
            big_blind: 150,
            my_chips: 9000,
            current_bet: 600,
            ..Default::default()
        },
        &strategy_map,
        &tracker,
        None,
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
    let (act_vs_3bet, _, _) = decide_action_traced(
        &LiveDecisionRequest {
            hole: hole_j6s,
            board: &board_583,
            legal: &legal,
            to_call: 3750,
            pot: 7500,
            history,
            my_player: 1,
            contributions: [300, 300],
            raises_this_round: 1,
            big_blind: 150,
            my_chips: 8000,
            current_bet: 3750,
            ..Default::default()
        },
        &strategy_map,
        &tracker,
        None,
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
    let (act, _, _) = decide_action_traced(
        &LiveDecisionRequest {
            hole: hole_76s,
            legal: &legal,
            to_call: 400,
            pot: 1200,
            history,
            contributions: [400, 800],
            big_blind: 800,
            my_chips: 10000,
            current_bet: 800,
            ..Default::default()
        },
        &strategy_map,
        &tracker,
        None,
    );
    assert_ne!(
        act, "allin",
        "76s at 12.5 BB (10,000 chips @ 400/800) must not open-jam all-in"
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
    let (tag_act, _, _) = decide_action_traced(
        &LiveDecisionRequest {
            hole: hole_aqo,
            legal: &legal,
            to_call: 6600,
            pot: 7000,
            history,
            my_player: 1,
            contributions: [200, 200],
            raises_this_round: 1,
            big_blind: 200,
            my_chips: 6600,
            current_bet: 6600,
            ..Default::default()
        },
        &strategy_map,
        &tag_tracker,
        None,
    );
    assert_eq!(tag_act, "fold", "AQo must fold vs non-maniac 33 BB shove");

    /* 2. TAG opponent at 28 BB: within HU Nash call cap (28 <= 30 BB), mixed/majority call */
    let mut hu_28_calls = 0usize;
    for _ in 0..30 {
        let (hu_28_act, _, _) = decide_action_traced(
            &LiveDecisionRequest {
                hole: hole_aqo,
                legal: &legal,
                to_call: 5600,
                pot: 6000,
                history,
                my_player: 1,
                contributions: [200, 200],
                raises_this_round: 1,
                big_blind: 200,
                my_chips: 5600,
                current_bet: 5600,
                ..Default::default()
            },
            &strategy_map,
            &tag_tracker,
            None,
        );
        if hu_28_act != "fold" {
            hu_28_calls += 1;
        }
    }
    assert!(
        hu_28_calls >= 12,
        "AQo should frequently call <= 30 BB HU shove (got {hu_28_calls}/30 calls)"
    );

    /* 3. Maniac opponent at 33 BB: expanded maniac cap (33 <= 38 BB) calls */
    let mut maniac_tracker = OpponentTracker::new();
    for _ in 0..16 {
        maniac_tracker.record_action_street_detailed(ALL_IN, 1, false, 4000, 600);
        maniac_tracker.record_preflop_positional(true, true, true);
        maniac_tracker.end_hand();
    }
    let (maniac_act, _, _) = decide_action_traced(
        &LiveDecisionRequest {
            hole: hole_aqo,
            legal: &legal,
            to_call: 6600,
            pot: 7000,
            history,
            my_player: 1,
            contributions: [200, 200],
            raises_this_round: 1,
            big_blind: 200,
            my_chips: 6600,
            current_bet: 6600,
            ..Default::default()
        },
        &strategy_map,
        &maniac_tracker,
        None,
    );
    assert_ne!(maniac_act, "fold", "AQo should call maniac 33 BB shove");
}

#[test]
fn test_preflop_micro_stack_1chip_calls() {
    let tracker = OpponentTracker::new();
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();

    /* Micro-stack preflop commitment: Kd Qc with 1 chip remaining facing 1-chip effective call into 5,248 pot
     * must return "call" (NEVER "fold" and NEVER "allin"/"raise" when to_call >= my_chips) */
    let hole_kqo = [
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Queen, Suit::Clubs),
    ];
    let legal_1chip = vec!["fold".to_string(), "call".to_string(), "allin".to_string()];
    let (act_1chip, _, tr_1chip) = decide_action_traced(
        &LiveDecisionRequest {
            hole: hole_kqo,
            legal: &legal_1chip,
            to_call: 1,
            pot: 5248,
            history,
            my_player: 1,
            contributions: [2624, 2624],
            raises_this_round: 1,
            big_blind: 400,
            my_chips: 1,
            current_bet: 13124,
            ..Default::default()
        },
        &strategy_map,
        &tracker,
        None,
    );
    assert_eq!(
        act_1chip, "call",
        "With 1 chip remaining facing 1 chip to call in 5248 pot, must return 'call' (reasons: {:?})",
        tr_1chip.guard_reasons
    );
}

#[test]
fn test_flop_oop_donk_suppresses_min_and_third_pot() {
    use lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket;

    let tracker = OpponentTracker::new();
    let mut history = [RoundHistory::new(); NUM_ROUNDS];
    history[0].push(RAISE_MIN);
    history[0].push(CALL_CHECK);

    let hole = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Six, Suit::Hearts),
    ];
    let board = [
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Jack, Suit::Spades),
        Card::new(Rank::Eight, Suit::Clubs),
    ];
    let bucket = postflop_equity_bucket(&hole, &board);
    let key = format!("P:rc|F:B{bucket:02}/");
    let legal = vec![
        "fold".to_string(),
        "check".to_string(),
        "bet".to_string(),
        "allin".to_string(),
    ];

    for probs in [
        vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    ] {
        let mut strategy_map = HashMap::new();
        strategy_map.insert(key.clone(), probs);
        let (act, amt, tr) = decide_action_traced(
            &LiveDecisionRequest {
                hole,
                board: &board,
                legal: &legal,
                pot: 800,
                subgame_search: false,
                history,
                my_player: 1,
                contributions: [400, 400],
                big_blind: 200,
                my_chips: 4400,
                opp_chips: Some(15000),
                ..Default::default()
            },
            &strategy_map,
            &tracker,
            None,
        );
        assert_eq!(
            act, "check",
            "OOP flop micro/33% donk with bucket {bucket} into PF aggressor must suppress to check (reasons: {:?})",
            tr.guard_reasons
        );
        assert_eq!(amt, 0);
        assert!(tr.guard_reasons.contains(&"FLOP_OOP_MICRO_DONK_SUPPRESS"));
    }
}
