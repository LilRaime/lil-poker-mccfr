use lil_poker_mccfr::game::holdem::{
    Rank, Suit, ALL_IN, CALL_CHECK, FOLD, NUM_ROUNDS, RAISE_HALF_POT, RAISE_MIN,
};
use lil_poker_mccfr::live::protocol::{map_action_index, parse_card_str, parse_json_i32};
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
    let (act_free, amt_free) =
        map_action_index(FOLD as usize, &legal, 0, 100, false).expect("map index 0 when free");
    assert_eq!(act_free, "check");
    assert_eq!(amt_free, 0);

    /* Index 0 (Fold): Facing bet (to_call > 0) -> should fold */
    let (act_facing_bet, _) =
        map_action_index(FOLD as usize, &legal, 50, 100, false).expect("map index 0 facing bet");
    assert_eq!(act_facing_bet, "fold");

    /* Index 1 (Call / Check): When to_call > 0 -> call */
    let (act_call, amt_call) = map_action_index(CALL_CHECK as usize, &legal, 50, 100, false)
        .expect("map index 1 facing bet");
    assert_eq!(act_call, "call");
    assert_eq!(amt_call, 0);

    /* Index 4 (Raise Half Pot): dry vs wet board */
    let (_, amt_dry) =
        map_action_index(RAISE_HALF_POT as usize, &legal, 0, 1000, false).expect("raise half dry");
    let (_, amt_wet) =
        map_action_index(RAISE_HALF_POT as usize, &legal, 0, 1000, true).expect("raise half wet");
    assert!(
        amt_wet > amt_dry,
        "Wet board raise half-pot should size larger than dry board"
    );

    /* Index 5 (All-in) */
    let (act_allin, _) = map_action_index(ALL_IN as usize, &legal, 50, 100, false).expect("allin");
    assert_eq!(act_allin, "allin");
}

#[test]
fn test_live_map_action_index_round() {
    use lil_poker_mccfr::live::protocol::map_action_index_round;

    let legal = vec![
        "fold".to_string(),
        "check".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];

    /* On River (round 4), Index 4 should be full pot (1000 chips) */
    let (_, amt_river) = map_action_index_round(RAISE_HALF_POT as usize, &legal, 0, 1000, false, 4)
        .expect("river bet sizing");
    assert_eq!(amt_river, 1000, "River bet sizing must scale to full pot");

    /* On Turn (round 3), Index 4 should be 75% pot (750 chips) */
    let (_, amt_turn) = map_action_index_round(RAISE_HALF_POT as usize, &legal, 0, 1000, false, 3)
        .expect("turn bet sizing");
    assert_eq!(amt_turn, 750, "Turn bet sizing must scale to 75% pot");
}

#[test]
fn test_pseudo_harmonic_and_geometric_sizing() {
    use lil_poker_mccfr::game::holdem::calculate_geometric_bet_size;
    use lil_poker_mccfr::live::protocol::{pseudo_harmonic_action_mapping, pseudo_harmonic_prob};
    use rand::SeedableRng;

    /* 1. Geometric bet sizing commits stack smoothly */
    let pot = 1000;
    let stack = 4000;
    // 2 streets left (e.g. Turn & River): SPR = 4.0
    let bet_2streets = calculate_geometric_bet_size(pot, stack, 2);
    assert!(
        bet_2streets > 500 && bet_2streets < stack,
        "Geometric sizing should be reasonable fraction of pot, got {}",
        bet_2streets
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
fn test_live_decide_action_with_real_contributions() {
    use lil_poker_mccfr::cfr::fallback::StrategyLookupIndex;
    use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
    use lil_poker_mccfr::cfr::subgame::{SubgameContext, SubgameSolver};
    use lil_poker_mccfr::game::holdem::{Card, RoundHistory};
    use lil_poker_mccfr::live::decision::decide_action;
    use std::collections::HashMap;

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
fn test_live_state_parsing_and_round_tracker() {
    use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
    use lil_poker_mccfr::live::state::{parse_live_state, process_showdown, LiveRoundTracker};

    let state_json = json!({
        "hand_id": 42,
        "phase": "Flop",
        "active_player_id": "bot_123",
        "pot": 400,
        "current_bet": 200,
        "board": ["Qc", "Jc", "10c"],
        "players": [
            {
                "id": "bot_123",
                "hole": ["Ah", "Kd"],
                "bet": 100,
                "is_small_blind": true
            },
            {
                "id": "opp_999",
                "hole": ["9s", "8s"],
                "bet": 200,
                "is_small_blind": false
            }
        ],
        "legal_actions": ["fold", "call", "raise", "allin"]
    });

    let snapshot = parse_live_state(&state_json, "bot_123").expect("parse active bot state");
    assert_eq!(snapshot.hand_id, 42);
    assert_eq!(snapshot.phase, "Flop");
    assert_eq!(snapshot.board.len(), 3);
    assert_eq!(snapshot.hole[0].rank, Rank::Ace);
    assert_eq!(snapshot.hole[1].rank, Rank::King);
    assert_eq!(snapshot.to_call, 100);
    assert_eq!(snapshot.pot, 400);
    assert!(snapshot.my_is_sb);
    assert!(!snapshot.opp_is_sb);

    let mut tracker = OpponentTracker::new();
    let mut round_tracker = LiveRoundTracker::new();

    round_tracker.reset_hand(snapshot.hand_id);
    assert_eq!(round_tracker.current_hand_id, 42);

    let round_idx = round_tracker.update_street(snapshot.board.len());
    assert_eq!(round_idx, 1);

    round_tracker.infer_and_record_opponent_action(&snapshot, &mut tracker);
    assert!(!round_tracker.live_history[1].is_empty());

    let (contributions, raises) = round_tracker.compute_engine_contributions(&snapshot);
    assert_eq!(contributions[0] + contributions[1], snapshot.pot);
    assert!(raises >= 1);

    round_tracker.record_bot_action("call", 100, snapshot.pot);
    assert_eq!(round_tracker.live_history[1].len(), 2);

    /* Test process_showdown */
    let showdown_json = json!({
        "phase": "Showdown",
        "pot": 600,
        "board": ["Qc", "Jc", "10c", "2d", "3h"],
        "players": [
            {
                "id": "bot_123",
                "hole": ["Ah", "Kd"]
            },
            {
                "id": "opp_999",
                "hole": ["9s", "8s"]
            }
        ]
    });
    process_showdown(
        &showdown_json,
        "bot_123",
        &mut tracker,
        &round_tracker.live_history[3],
        round_tracker.last_opp_bet,
    );
    assert_eq!(tracker.showdown_hands, 1);
    tracker.end_hand();
    assert_eq!(tracker.total_hands, 1);
}

#[test]
fn test_derive_legal_actions_and_live_state_without_legal_actions() {
    use lil_poker_mccfr::live::state::{derive_legal_actions, parse_live_state};

    /* 1. derive_legal_actions when to_call == 0 */
    let acts_free = derive_legal_actions(0, 9600);
    assert!(acts_free.contains(&"check".to_string()));
    assert!(acts_free.contains(&"raise".to_string()));
    assert!(acts_free.contains(&"bet".to_string()));
    assert!(acts_free.contains(&"all_in".to_string()));

    /* 2. derive_legal_actions when to_call > 0 */
    let acts_facing_bet = derive_legal_actions(50, 9600);
    assert!(acts_facing_bet.contains(&"fold".to_string()));
    assert!(acts_facing_bet.contains(&"call".to_string()));
    assert!(acts_facing_bet.contains(&"raise".to_string()));
    assert!(acts_facing_bet.contains(&"bet".to_string()));
    assert!(acts_facing_bet.contains(&"all_in".to_string()));

    /* 3. Real lil-poker Go backend state (which never provides legal_actions) */
    let live_server_json = json!({
        "hand_count": 20,
        "phase": "Turn",
        "pot": 200,
        "current_bet": 0,
        "big_blind": 100,
        "small_blind": 50,
        "board": ["Qd", "10c", "Js", "9d"],
        "active_player_id": "CFR_Bot_775",
        "players": [
            {
                "id": "qqqq",
                "name": "qqqq",
                "chips": 10400,
                "bet": 0,
                "is_small_blind": true
            },
            {
                "id": "CFR_Bot_775",
                "name": "CFR_Bot_775",
                "chips": 9600,
                "bet": 0,
                "hole": ["10s", "8d"],
                "is_small_blind": false
            }
        ]
    });

    let snapshot = parse_live_state(&live_server_json, "CFR_Bot_775")
        .expect("must parse live state even without legal_actions");
    assert_eq!(snapshot.to_call, 0);
    assert_eq!(snapshot.pot, 200);
    assert_eq!(snapshot.my_chips, 9600);
    assert_eq!(snapshot.opp_chips, 10400);
    assert!(snapshot.legal_actions.contains(&"raise".to_string()));
    assert!(snapshot.legal_actions.contains(&"check".to_string()));
}

#[test]
fn test_bot_value_bets_straight_in_live_game() {
    use lil_poker_mccfr::cfr::fallback::StrategyLookupIndex;
    use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
    use lil_poker_mccfr::cfr::subgame::{SubgameContext, SubgameSolver};
    use lil_poker_mccfr::game::holdem::{Card, RoundHistory};
    use lil_poker_mccfr::live::decision::decide_action;
    use lil_poker_mccfr::live::state::derive_legal_actions;
    use std::collections::HashMap;

    /* Recreate exact hand from user screenshot:
     * Bot holds [10s, 8d], Board on Turn is [Qd, 10c, Js, 9d] -> Straight, Queen High! */
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
    let history = [
        RoundHistory::new(),
        RoundHistory::new(),
        RoundHistory::new(),
        RoundHistory::new(),
    ];
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
            true, // subgame search enabled
            &tracker,
            &history,
            1, // BB (my_player)
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

    /* With Queen-high Straight, bot should value bet / raise aggressively (> 50% of the time, GTO mixed strategy) */
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
fn test_call_check_facing_bet_never_checks() {
    use lil_poker_mccfr::game::holdem::CALL_CHECK;
    use lil_poker_mccfr::live::protocol::map_action_index_round;

    let legal = vec![
        "fold".to_string(),
        "call".to_string(),
        "check".to_string(),
        "raise".to_string(),
    ];

    /* When facing a bet (to_call > 0), CALL_CHECK action index MUST map to "call" */
    let (act, amt) = map_action_index_round(CALL_CHECK as usize, &legal, 50, 150, false, 1)
        .expect("map call_check");
    assert_eq!(act, "call", "Facing bet must map to call, not check!");
    assert_eq!(amt, 0);

    /* When free (to_call == 0), CALL_CHECK action index MUST map to "check" */
    let (act_free, amt_free) =
        map_action_index_round(CALL_CHECK as usize, &legal, 0, 150, false, 2)
            .expect("map call_check when free");
    assert_eq!(
        act_free, "check",
        "Free action must map to check, not call!"
    );
    assert_eq!(amt_free, 0);
}
