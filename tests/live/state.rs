use lil_poker_mccfr::cfr::abstraction::get_holdem_infoset_key_rich;
use lil_poker_mccfr::cfr::fallback::lookup_strategy_hierarchical;
use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
use lil_poker_mccfr::game::holdem::{
    Card, Rank, RoundHistory, Suit, CALL_CHECK, NUM_ROUNDS, RAISE_HALF_POT,
};
use lil_poker_mccfr::live::decision::{decide_action_multiway, decide_action_multiway_context};
use lil_poker_mccfr::live::state::{
    derive_legal_actions, parse_live_state, parse_live_state_with_hint, process_showdown,
    LiveRoundTracker, LiveTableTracker,
};
use serde_json::json;
use std::collections::HashMap;

#[test]
fn test_live_state_parsing_and_round_tracker() {
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

    /* 3. Backend state without explicit legal_actions */
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
fn test_live_street_transition_closes_preflop_for_rich_keys() {
    let mut tracker = LiveRoundTracker::new();
    tracker.reset_hand(1);

    /* Bot is SB, raises preflop */
    tracker.record_bot_action("raise", 200, 150);
    assert_eq!(tracker.live_history[0].as_slice(), &[RAISE_HALF_POT]);

    /* Opponent calls, dealer deals flop (3 board cards) */
    let new_round = tracker.update_street(3);
    assert_eq!(new_round, 1); /* Flop index */
    /* Previous preflop street MUST be closed with CALL_CHECK! */
    assert_eq!(
        tracker.live_history[0].as_slice(),
        &[RAISE_HALF_POT, CALL_CHECK]
    );

    /* Test rich key generation on flop */
    let hole = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
    ];
    let board = [
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Hearts),
    ];
    let rich_key = get_holdem_infoset_key_rich(&hole, &board, 2, &tracker.live_history);
    assert!(
        rich_key.starts_with("P:rc|F:B"),
        "Flop key must start with P:rc|F:B, got: {}",
        rich_key
    );

    /* Test hierarchical strategy lookup succeeds */
    let mut strat_map: HashMap<String, Vec<f64>> = HashMap::new();
    strat_map.insert(rich_key.clone(), vec![0.0, 0.4, 0.3, 0.2, 0.1, 0.0]);

    let res = lookup_strategy_hierarchical(
        |k| strat_map.get(k),
        &hole,
        &board,
        2,
        &tracker.live_history,
    );
    assert!(res.is_some(), "Hierarchical lookup must find rich key!");
}

#[test]
fn test_multiway_6max_state_and_decisions() {
    /* 6-player table where 2 opponents folded and 3 opponents remain active */
    let state_6max = json!({
        "hand_id": 101,
        "phase": "Flop",
        "active_player_id": "bot_hero",
        "pot": 1200,
        "current_bet": 400,
        "board": ["Kh", "9d", "4c"],
        "players": [
            { "id": "p1", "name": "utg_fold", "bet": 0, "chips": 9900, "folded": true },
            { "id": "p2", "name": "mp_raiser", "bet": 400, "chips": 8500, "folded": false },
            { "id": "p3", "name": "co_caller", "bet": 400, "chips": 7200, "folded": false },
            { "id": "bot_hero", "hole": ["7s", "2d"], "bet": 0, "chips": 9600, "is_small_blind": true },
            { "id": "p5", "name": "bb_active", "bet": 0, "chips": 9000, "is_big_blind": true, "folded": false },
            { "id": "p6", "name": "btn_folded", "bet": 0, "chips": 10000, "status": "folded" }
        ],
        "legal_actions": ["fold", "call", "raise", "allin"]
    });

    let snap = parse_live_state(&state_6max, "bot_hero").expect("parse 6max state");
    assert_eq!(
        snap.active_opponents, 3,
        "Should detect exactly 3 active non-folded opponents"
    );
    assert_eq!(
        snap.opp_bet, 400,
        "Primary opponent bet must be highest active bet (400), not overwritten by last folded player (0)!"
    );
    assert_eq!(snap.opp_name, "mp_raiser");
    assert_eq!(
        snap.my_player, 1,
        "SB in multi-way pot acts OOP postflop (player 1)"
    );

    /* Pure air (72o on K-9-4) facing a bet in a 4-way pot must fold */
    let tracker = OpponentTracker::new();
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let (act, _) = decide_action_multiway(
        &snap.hole,
        &snap.board,
        &snap.legal_actions,
        snap.to_call,
        snap.pot,
        &strategy_map,
        true,
        &tracker,
        &history,
        snap.my_player,
        [200, 600],
        1,
        None,
        None,
        None,
        snap.active_opponents,
    );
    assert_eq!(act, "fold");

    /* Multi-way Showdown with 2 opponents showing cards must record both */
    let showdown_6max = json!({
        "phase": "Showdown",
        "pot": 2400,
        "board": ["Kh", "9d", "4c", "2s", "Jc"],
        "players": [
            { "id": "bot_hero", "hole": ["Ah", "Kd"] },
            { "id": "p2", "hole": ["Ks", "Qd"] },
            { "id": "p3", "hole": ["7d", "2c"] }
        ]
    });
    let mut sd_tracker = OpponentTracker::new();
    let river_hist = RoundHistory::new();
    process_showdown(
        &showdown_6max,
        "bot_hero",
        &mut sd_tracker,
        &river_hist,
        400,
    );
    assert_eq!(
        sd_tracker.showdown_hands, 2,
        "Multi-way showdown must record each opponent's hand individually"
    );
    let _ = Card::new(Rank::Ace, Suit::Spades);
}

#[test]
fn test_uncallable_overbet_pot_preservation_and_bb_defense() {
    /* 1. When server pot does not yet include an opponent's massive overbet jam (e.g. pot = 9112, opp bet = 10887, hero chips = 1),
     * parse_live_state must NOT wipe snapshot.pot to 0 via saturating_sub! */
    let overbet_json = json!({
        "type": "state",
        "hand": 10,
        "round": "flop",
        "pot": 9112,
        "current_bet": 10887,
        "big_blind": 200,
        "active_player_id": "hero_bot",
        "board": ["Ks", "Kd", "6s"],
        "players": [
            { "id": "hero_bot", "name": "hero_bot", "chips": 1, "bet": 0, "folded": false, "all_in": false, "hole": ["9d", "7c"] },
            { "id": "villain", "name": "villain", "chips": 0, "bet": 10887, "folded": false, "all_in": true }
        ]
    });
    let snap = parse_live_state(&overbet_json, "hero_bot").expect("must parse state");
    assert_eq!(snap.to_call, 1);
    assert_eq!(
        snap.pot, 9112,
        "Pot must not be wiped to zero when uncallable overbet exceeds raw pot"
    );

    let tracker = OpponentTracker::new();
    let history = [RoundHistory::new(); NUM_ROUNDS];
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let hole_97o = [
        Card::new(Rank::Nine, Suit::Diamonds),
        Card::new(Rank::Seven, Suit::Clubs),
    ];
    let board_kk6 = vec![
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Spades),
    ];
    let legal_call = vec!["fold".to_string(), "call".to_string(), "allin".to_string()];
    let (act_one_chip, _) = decide_action_multiway_context(
        &hole_97o,
        &board_kk6,
        &legal_call,
        snap.to_call,
        snap.pot,
        &strategy_map,
        true,
        &tracker,
        &history,
        0,
        [4556, 4557],
        0,
        None,
        None,
        None,
        1,
        200,
        snap.my_chips,
    );
    assert_ne!(
        act_one_chip, "fold",
        "Must never fold for 1 chip into a 9,112 pot!"
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
    let (act_bb_defend, _) = decide_action_multiway_context(
        &hole_a4o,
        &[],
        &legal_pf,
        200,
        600,
        &strategy_map,
        true,
        &tracker,
        &history,
        1,
        [200, 400],
        0,
        None,
        None,
        None,
        1,
        200,
        1950,
    );
    assert_eq!(
        act_bb_defend, "call",
        "In BB at ~10 BB facing a 2x min-raise with A4o, bot should flat-call rather than jam all-in"
    );
}

#[test]
fn test_live_table_tracker_6max_individual_tracking_and_finalize() {
    let mut table = LiveTableTracker::new();
    let mut trackers: HashMap<String, OpponentTracker> = HashMap::new();

    /* Step 1: Preflop start with 4 seats (Hero in BB, VillainA opens, VillainB in SB hasn't acted yet, VillainC acts first) */
    let s1 = json!({
        "hand_id": 1001,
        "phase": "Preflop",
        "dealer_seat": 1,
        "pot": 150,
        "current_bet": 100,
        "players": [
            { "id": "Hero", "name": "Hero", "seat": 3, "chips": 9900, "bet": 100, "is_big_blind": true, "folded": false },
            { "id": "VillainA", "name": "VillainA", "seat": 1, "chips": 10000, "bet": 0, "folded": false },
            { "id": "VillainB", "name": "VillainB", "seat": 2, "chips": 9950, "bet": 50, "is_small_blind": true, "folded": false },
            { "id": "VillainC", "name": "VillainC", "seat": 0, "chips": 10000, "bet": 0, "folded": false }
        ]
    });
    table.observe_state(&s1, "Hero", &mut trackers, None);

    /* Step 2: VillainC opens to 300 (PFR+VPIP), VillainA calls 300 (VPIP only), VillainB folds, now it's Hero's turn */
    let s2 = json!({
        "hand_id": 1001,
        "phase": "Preflop",
        "dealer_seat": 1,
        "pot": 800,
        "current_bet": 300,
        "players": [
            { "id": "VillainC", "name": "VillainC", "seat": 0, "chips": 9700, "bet": 300, "folded": false },
            { "id": "VillainA", "name": "VillainA", "seat": 1, "chips": 9700, "bet": 300, "folded": false },
            { "id": "VillainB", "name": "VillainB", "seat": 2, "chips": 9950, "bet": 50, "is_small_blind": true, "folded": true },
            { "id": "Hero", "name": "Hero", "seat": 3, "chips": 9900, "bet": 100, "is_big_blind": true, "folded": false }
        ]
    });
    table.observe_state(&s2, "Hero", &mut trackers, None);

    /* Step 3: Hero 3-bets to 900 */
    let s3 = json!({
        "hand_id": 1001,
        "phase": "Preflop",
        "dealer_seat": 1,
        "pot": 1600,
        "current_bet": 900,
        "players": [
            { "id": "Hero", "name": "Hero", "seat": 3, "chips": 9100, "bet": 900, "is_big_blind": true, "folded": false },
            { "id": "VillainA", "name": "VillainA", "seat": 1, "chips": 9700, "bet": 300, "folded": false },
            { "id": "VillainB", "name": "VillainB", "seat": 2, "chips": 9950, "bet": 50, "is_small_blind": true, "folded": true },
            { "id": "VillainC", "name": "VillainC", "seat": 0, "chips": 9700, "bet": 300, "folded": false }
        ]
    });
    table.observe_state(&s3, "Hero", &mut trackers, None);

    /* Step 4: VillainC folds to Hero's 3-bet right as server deals Flop; VillainA called 900 */
    let s4 = json!({
        "hand_id": 1001,
        "phase": "Flop",
        "dealer_seat": 1,
        "pot": 2150,
        "current_bet": 0,
        "board": ["As", "Kd", "4c"],
        "players": [
            { "id": "Hero", "name": "Hero", "seat": 3, "chips": 9100, "bet": 0, "is_big_blind": true, "folded": false },
            { "id": "VillainA", "name": "VillainA", "seat": 1, "chips": 9100, "bet": 0, "folded": false },
            { "id": "VillainB", "name": "VillainB", "seat": 2, "chips": 9950, "bet": 0, "is_small_blind": true, "folded": true },
            { "id": "VillainC", "name": "VillainC", "seat": 0, "chips": 9700, "bet": 0, "folded": true }
        ]
    });
    table.observe_state(&s4, "Hero", &mut trackers, None);
    table.finalize_hand(&mut trackers, "");

    /* All three opponents must have total_hands == 1 tracked individually (not just one current_opp_name!) */
    let ta = trackers.get("VillainA").expect("VillainA tracked");
    let tb = trackers.get("VillainB").expect("VillainB tracked");
    let tc = trackers.get("VillainC").expect("VillainC tracked");
    assert_eq!(ta.total_hands, 1);
    assert_eq!(tb.total_hands, 1);
    assert_eq!(tc.total_hands, 1);

    /* VillainC raised to 300 first (PFR+VPIP), then folded to Hero's 3-bet */
    assert_eq!(tc.vpip_hands, 1);
    assert_eq!(tc.pfr_hands, 1);
    assert_eq!(tc.faced_3bet_count, 1);
    assert_eq!(tc.fold_to_3bet_count, 1);

    /* VillainA called 300 then called 900 -> VPIP, not PFR */
    assert_eq!(ta.vpip_hands, 1);
    assert_eq!(ta.pfr_hands, 0);

    /* VillainB posted SB and folded without putting extra chips in */
    assert_eq!(tb.sb_total_hands, 1);
    assert_eq!(tb.vpip_hands, 0);
}

#[test]
fn test_6max_unopened_history_and_blind_inference_without_flags() {
    /* 1. 6-max payload without explicit big_blind or is_small_blind/is_big_blind flags:
     * SB (150) and BB (300) posted, two early seats folded, Hero CO holds 9c 9h in unopened pot (pot = 450). */
    let s_6max = json!({
        "hand_id": 64,
        "phase": "Preflop",
        "pot": 450,
        "current_bet": 300,
        "active_player_id": "hero",
        "board": [],
        "players": [
            { "id": "p1", "name": "UtgBot", "chips": 9725, "bet": 0, "folded": true },
            { "id": "p2", "name": "MpBot", "chips": 7575, "bet": 0, "folded": true },
            { "id": "hero", "name": "HeroBot", "chips": 8938, "bet": 0, "hole": ["9c", "9h"] },
            { "id": "p4", "name": "BtnBot", "chips": 11746, "bet": 0 },
            { "id": "p5", "name": "SbBot", "chips": 8225, "bet": 150 },
            { "id": "p6", "name": "BbBot", "chips": 13341, "bet": 300 }
        ]
    });
    let snap = parse_live_state(&s_6max, "hero").expect("parse 6-max snapshot");
    assert_eq!(
        snap.big_blind, 300,
        "Should infer 300 big blind from untouched (150, 300) blinds without flags"
    );
    assert_eq!(snap.small_blind, 150);

    let mut rt = LiveRoundTracker::new();
    rt.reset_hand(snap.hand_id);
    rt.infer_opponent_history_only(&snap);
    assert!(
        rt.live_history[0].is_empty(),
        "Unopened 6-max pot before CO must keep preflop history empty, got {:?}",
        rt.live_history[0].as_slice()
    );

    /* 2. HU SB limp (400 == 400) & Flop blind hint preservation */
    let s_hu_limp = json!({
        "hand_id": 84,
        "phase": "Preflop",
        "pot": 800,
        "current_bet": 400,
        "active_player_id": "hero",
        "board": [],
        "players": [
            { "id": "opp", "name": "Villain", "chips": 16975, "bet": 400, "is_small_blind": true },
            { "id": "hero", "name": "HeroBot", "chips": 2225, "bet": 400, "is_big_blind": true, "hole": ["Kd", "Qc"] }
        ]
    });
    let snap_limp = parse_live_state(&s_hu_limp, "hero").expect("parse HU limp snapshot");
    assert_eq!(
        snap_limp.big_blind, 400,
        "BB should remain 400 after SB limps to match BB"
    );

    let s_flop_no_bb = json!({
        "hand_id": 84,
        "phase": "Flop",
        "pot": 1600,
        "current_bet": 0,
        "active_player_id": "hero",
        "board": ["As", "Kd", "4c"],
        "players": [
            { "id": "opp", "name": "Villain", "chips": 16575, "bet": 0, "is_small_blind": true },
            { "id": "hero", "name": "HeroBot", "chips": 1825, "bet": 0, "is_big_blind": true, "hole": ["Kd", "Qc"] }
        ]
    });
    let snap_flop =
        parse_live_state_with_hint(&s_flop_no_bb, "hero", snap_limp.big_blind).expect("parse flop");
    assert_eq!(
        snap_flop.big_blind, 400,
        "Same-hand hint should preserve 400 big blind on Flop"
    );
}
