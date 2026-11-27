use super::{board, hole};
use lil_poker_mccfr::cfr::abstraction::get_holdem_infoset_key_rich;
use lil_poker_mccfr::cfr::fallback::lookup_strategy_hierarchical;
use lil_poker_mccfr::cfr::opponent_model::{OpponentTracker, effective_remaining_chips};
use lil_poker_mccfr::game::config::RAISE_MIN;
use lil_poker_mccfr::game::holdem::{CALL_CHECK, NUM_ROUNDS, RAISE_HALF_POT, Rank, RoundHistory};
use lil_poker_mccfr::live::client::{
    EmergencyFallbackRequest, build_emergency_fallback_candidates,
};
use lil_poker_mccfr::live::decision::{LiveDecisionRequest, decide_action};
use lil_poker_mccfr::live::state::{
    LiveRoundTracker, LiveTableTracker, derive_legal_actions, parse_live_state,
    parse_live_state_with_hint, process_showdown,
};
use serde_json::{Value, json};
use std::collections::HashMap;

fn legal_list(acts: &[&str]) -> Vec<String> {
    acts.iter().map(|s| (*s).to_string()).collect()
}

fn observe_hu_n(
    rt: &mut LiveRoundTracker,
    tr: &mut OpponentTracker,
    ids: std::ops::RangeInclusive<u64>,
    mut tpl: Value,
) {
    for id in ids {
        tpl["hand_id"] = json!(id);
        let snap = parse_live_state(&tpl, "hero").expect("parse live snapshot");
        rt.reset_hand(snap.hand_id);
        rt.update_street(snap.board.len());
        rt.infer_and_record_opponent_action(&snap, tr);
        tr.end_hand();
    }
}

fn cands(
    legal: &[String],
    first_retry: Option<(&str, i32)>,
    err_text: &str,
    to_call: i32,
    my_chips: i32,
) -> Vec<(&'static str, i32)> {
    build_emergency_fallback_candidates(&EmergencyFallbackRequest {
        legal_actions: legal,
        chosen_action: "call",
        chosen_amount: 0,
        first_retry,
        err_text,
        to_call,
        my_chips,
    })
}

#[test]
fn test_live_state_parsing_and_round_tracker() {
    let state_json = json!({
        "hand_id": 42, "phase": "Flop", "active_player_id": "bot_123", "pot": 400, "current_bet": 200, "board": ["Qc", "Jc", "10c"],
        "players": [{ "id": "bot_123", "hole": ["Ah", "Kd"], "bet": 100, "is_small_blind": true }, { "id": "opp_999", "hole": ["9s", "8s"], "bet": 200, "is_small_blind": false }],
        "legal_actions": ["fold", "call", "raise", "allin"]
    });
    let s = parse_live_state(&state_json, "bot_123").expect("parse active bot state");
    assert_eq!(s.hand_id, 42);
    assert_eq!(s.phase, "Flop");
    assert_eq!(s.board.len(), 3);
    assert_eq!((s.hole[0].rank, s.hole[1].rank), (Rank::Ace, Rank::King));
    assert_eq!((s.to_call, s.pot), (100, 400));
    assert!(s.my_is_sb && !s.opp_is_sb);

    let (mut tracker, mut rt) = (OpponentTracker::new(), LiveRoundTracker::new());
    rt.reset_hand(s.hand_id);
    assert_eq!(rt.current_hand_id, 42);
    assert_eq!(rt.update_street(s.board.len()), 1);
    rt.infer_and_record_opponent_action(&s, &mut tracker);
    assert!(!rt.live_history[1].is_empty());

    let (contributions, raises) = rt.compute_engine_contributions(&s);
    assert!(contributions[0] + contributions[1] == s.pot && raises >= 1);
    rt.record_bot_action("call", 100, s.pot);
    assert_eq!(rt.live_history[1].len(), 2);

    let sd_json = json!({
        "phase": "Showdown", "pot": 600, "board": ["Qc", "Jc", "10c", "2d", "3h"],
        "players": [{ "id": "bot_123", "hole": ["Ah", "Kd"] }, { "id": "opp_999", "hole": ["9s", "8s"] }]
    });
    let r_hist = &rt.live_history[3];
    process_showdown(&sd_json, "bot_123", &mut tracker, r_hist, rt.last_opp_bet);
    assert_eq!(tracker.showdown_hands, 1);
    tracker.end_hand();
    assert_eq!(tracker.total_hands, 1);
}

#[test]
fn test_derive_legal_actions_and_live_state_without_legal_actions() {
    let (free, bet) = (
        derive_legal_actions(0, 9600),
        derive_legal_actions(50, 9600),
    );
    for a in ["check", "raise", "bet", "all_in"] {
        assert!(free.iter().any(|x| x == a));
    }
    for a in ["fold", "call", "raise", "bet", "all_in"] {
        assert!(bet.iter().any(|x| x == a));
    }

    let live_server_json = json!({
        "hand_count": 20, "phase": "Turn", "pot": 200, "current_bet": 0, "big_blind": 100, "small_blind": 50,
        "board": ["Qd", "10c", "Js", "9d"], "active_player_id": "CFR_Bot_775",
        "players": [
            { "id": "qqqq", "name": "qqqq", "chips": 10400, "bet": 0, "is_small_blind": true },
            { "id": "CFR_Bot_775", "name": "CFR_Bot_775", "chips": 9600, "bet": 0, "hole": ["10s", "8d"], "is_small_blind": false }
        ]
    });
    let s = parse_live_state(&live_server_json, "CFR_Bot_775").expect("parse without legal");
    assert_eq!((s.to_call, s.pot), (0, 200));
    assert_eq!((s.my_chips, s.opp_chips), (9600, Some(10400)));
    assert!(s.legal_actions.iter().any(|a| a == "raise"));
    assert!(s.legal_actions.iter().any(|a| a == "check"));
}

#[test]
fn test_live_street_transition_closes_preflop_for_rich_keys() {
    let mut tracker = LiveRoundTracker::new();
    tracker.reset_hand(1);
    tracker.record_bot_action("raise", 200, 150);
    assert_eq!(tracker.live_history[0].as_slice(), &[RAISE_HALF_POT]);
    assert_eq!(tracker.update_street(3), 1);
    let pf = tracker.live_history[0].as_slice();
    assert_eq!(pf, &[RAISE_HALF_POT, CALL_CHECK]);

    let (h, b) = (hole("As Kh"), board("Ac 7d 2h"));
    let rich_key = get_holdem_infoset_key_rich(&h, &b, 2, &tracker.live_history);
    assert!(rich_key.starts_with("P:rc|F:B"), "Flop key: {rich_key}");

    let mut strat_map: HashMap<String, Vec<f64>> = HashMap::new();
    strat_map.insert(rich_key, vec![0.0, 0.4, 0.3, 0.2, 0.1, 0.0]);
    let found =
        lookup_strategy_hierarchical(|k| strat_map.get(k), &h, &b, 2, &tracker.live_history);
    assert!(found.is_some());
}

#[test]
fn test_multiway_6max_state_and_decisions() {
    let state_6max = json!({
        "hand_id": 101, "phase": "Flop", "active_player_id": "bot_hero", "pot": 1200, "current_bet": 400, "board": ["Kh", "9d", "4c"],
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
    let s = parse_live_state(&state_6max, "bot_hero").expect("parse 6max state");
    assert_eq!((s.active_opponents, s.opp_bet, s.my_player), (3, 400, 1));
    assert_eq!(s.opp_name, "mp_raiser");

    let (tracker, history) = (OpponentTracker::new(), [RoundHistory::new(); NUM_ROUNDS]);
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let req = LiveDecisionRequest {
        hole: s.hole,
        board: &s.board,
        legal: &s.legal_actions,
        to_call: s.to_call,
        pot: s.pot,
        subgame_search: true,
        history,
        my_player: s.my_player,
        contributions: [200, 600],
        raises_this_round: 1,
        active_opponents: s.active_opponents,
        ..Default::default()
    };
    assert_eq!(decide_action(&req, &strategy_map, &tracker, None).0, "fold");

    let sd_6max = json!({
        "phase": "Showdown", "pot": 2400, "board": ["Kh", "9d", "4c", "2s", "Jc"],
        "players": [{ "id": "bot_hero", "hole": ["Ah", "Kd"] }, { "id": "p2", "hole": ["Ks", "Qd"] }, { "id": "p3", "hole": ["7d", "2c"] }]
    });
    let mut sd_tr = OpponentTracker::new();
    process_showdown(&sd_6max, "bot_hero", &mut sd_tr, &RoundHistory::new(), 400);
    assert_eq!(sd_tr.showdown_hands, 2);
}

#[test]
fn test_uncallable_overbet_pot_preservation_and_one_chip_call() {
    let overbet_json = json!({
        "type": "state", "hand": 10, "round": "flop", "pot": 9112, "current_bet": 10887,
        "big_blind": 200, "active_player_id": "hero_bot", "board": ["Ks", "Kd", "6s"],
        "players": [
            { "id": "hero_bot", "name": "hero_bot", "chips": 1, "bet": 0, "folded": false, "all_in": false, "hole": ["9d", "7c"] },
            { "id": "villain", "name": "villain", "chips": 0, "bet": 10887, "folded": false, "all_in": true }
        ]
    });
    let s = parse_live_state(&overbet_json, "hero_bot").expect("must parse state");
    assert_eq!((s.to_call, s.pot), (1, 9112));

    let (tracker, history) = (OpponentTracker::new(), [RoundHistory::new(); NUM_ROUNDS]);
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let (legal_call, b) = (legal_list(&["fold", "call", "allin"]), board("Ks Kd 6s"));
    let req = LiveDecisionRequest {
        hole: hole("9d 7c"),
        board: &b,
        legal: &legal_call,
        to_call: s.to_call,
        pot: s.pot,
        subgame_search: true,
        history,
        contributions: [4556, 4557],
        big_blind: 200,
        my_chips: s.my_chips,
        ..Default::default()
    };
    assert_ne!(decide_action(&req, &strategy_map, &tracker, None).0, "fold");
}

#[test]
fn test_live_table_tracker_6max_individual_tracking_and_finalize() {
    let mut table = LiveTableTracker::new();
    let mut trackers: HashMap<String, OpponentTracker> = HashMap::new();
    for p in [
        json!({ "hand_id": 1001, "phase": "Preflop", "dealer_seat": 1, "pot": 150, "current_bet": 100, "players": [{ "id": "Hero", "name": "Hero", "seat": 3, "chips": 9900, "bet": 100, "is_big_blind": true, "folded": false }, { "id": "VillainA", "name": "VillainA", "seat": 1, "chips": 10000, "bet": 0, "folded": false }, { "id": "VillainB", "name": "VillainB", "seat": 2, "chips": 9950, "bet": 50, "is_small_blind": true, "folded": false }, { "id": "VillainC", "name": "VillainC", "seat": 0, "chips": 10000, "bet": 0, "folded": false }] }),
        json!({ "hand_id": 1001, "phase": "Preflop", "dealer_seat": 1, "pot": 800, "current_bet": 300, "players": [{ "id": "VillainC", "name": "VillainC", "seat": 0, "chips": 9700, "bet": 300, "folded": false }, { "id": "VillainA", "name": "VillainA", "seat": 1, "chips": 9700, "bet": 300, "folded": false }, { "id": "VillainB", "name": "VillainB", "seat": 2, "chips": 9950, "bet": 50, "is_small_blind": true, "folded": true }, { "id": "Hero", "name": "Hero", "seat": 3, "chips": 9900, "bet": 100, "is_big_blind": true, "folded": false }] }),
        json!({ "hand_id": 1001, "phase": "Preflop", "dealer_seat": 1, "pot": 1600, "current_bet": 900, "players": [{ "id": "Hero", "name": "Hero", "seat": 3, "chips": 9100, "bet": 900, "is_big_blind": true, "folded": false }, { "id": "VillainA", "name": "VillainA", "seat": 1, "chips": 9700, "bet": 300, "folded": false }, { "id": "VillainB", "name": "VillainB", "seat": 2, "chips": 9950, "bet": 50, "is_small_blind": true, "folded": true }, { "id": "VillainC", "name": "VillainC", "seat": 0, "chips": 9700, "bet": 300, "folded": false }] }),
        json!({ "hand_id": 1001, "phase": "Flop", "dealer_seat": 1, "pot": 2150, "current_bet": 0, "board": ["As", "Kd", "4c"], "players": [{ "id": "Hero", "name": "Hero", "seat": 3, "chips": 9100, "bet": 0, "is_big_blind": true, "folded": false }, { "id": "VillainA", "name": "VillainA", "seat": 1, "chips": 9100, "bet": 0, "folded": false }, { "id": "VillainB", "name": "VillainB", "seat": 2, "chips": 9950, "bet": 0, "is_small_blind": true, "folded": true }, { "id": "VillainC", "name": "VillainC", "seat": 0, "chips": 9700, "bet": 0, "folded": true }] }),
    ] {
        table.observe_state(&p, "Hero", &mut trackers, None);
    }
    table.finalize_hand(&mut trackers, "");

    let ta = trackers.get("VillainA").expect("VillainA tracked");
    let tb = trackers.get("VillainB").expect("VillainB tracked");
    let tc = trackers.get("VillainC").expect("VillainC tracked");
    assert_eq!((ta.total_hands, tb.total_hands, tc.total_hands), (1, 1, 1));
    assert_eq!((tc.vpip_hands, tc.pfr_hands), (1, 1));
    assert_eq!((tc.faced_3bet_count, tc.fold_to_3bet_count), (1, 1));
    assert_eq!((ta.vpip_hands, ta.pfr_hands), (1, 0));
    assert_eq!((tb.sb_total_hands, tb.vpip_hands), (1, 0));
}

#[test]
fn test_6max_unopened_history_and_blind_inference_without_flags() {
    let s_6max = json!({
        "hand_id": 64, "phase": "Preflop", "pot": 450, "current_bet": 300, "active_player_id": "hero", "board": [],
        "players": [
            { "id": "p1", "name": "UtgBot", "chips": 9725, "bet": 0, "folded": true },
            { "id": "p2", "name": "MpBot", "chips": 7575, "bet": 0, "folded": true },
            { "id": "hero", "name": "HeroBot", "chips": 8938, "bet": 0, "hole": ["9c", "9h"] },
            { "id": "p4", "name": "BtnBot", "chips": 11746, "bet": 0 },
            { "id": "p5", "name": "SbBot", "chips": 8225, "bet": 150 },
            { "id": "p6", "name": "BbBot", "chips": 13341, "bet": 300 }
        ]
    });
    let s = parse_live_state(&s_6max, "hero").expect("parse 6-max snapshot");
    assert_eq!((s.big_blind, s.small_blind), (300, 150));

    let mut rt = LiveRoundTracker::new();
    rt.reset_hand(s.hand_id);
    rt.infer_opponent_history_only(&s);
    assert!(rt.live_history[0].is_empty(), "Unopened 6-max before CO");

    let s_hu_limp = json!({
        "hand_id": 84, "phase": "Preflop", "pot": 800, "current_bet": 400, "active_player_id": "hero", "board": [],
        "players": [{ "id": "opp", "name": "Villain", "chips": 16975, "bet": 400, "is_small_blind": true }, { "id": "hero", "name": "HeroBot", "chips": 2225, "bet": 400, "is_big_blind": true, "hole": ["Kd", "Qc"] }]
    });
    let snap_limp = parse_live_state(&s_hu_limp, "hero").expect("parse HU limp");
    assert_eq!(snap_limp.big_blind, 400);

    let s_flop_no_bb = json!({
        "hand_id": 84, "phase": "Flop", "pot": 1600, "current_bet": 0, "active_player_id": "hero", "board": ["As", "Kd", "4c"],
        "players": [{ "id": "opp", "name": "Villain", "chips": 16575, "bet": 0, "is_small_blind": true }, { "id": "hero", "name": "HeroBot", "chips": 1825, "bet": 0, "is_big_blind": true, "hole": ["Kd", "Qc"] }]
    });
    let snap_flop = parse_live_state_with_hint(&s_flop_no_bb, "hero", snap_limp.big_blind);
    assert_eq!(snap_flop.expect("parse flop").big_blind, 400);
}

#[test]
fn test_hu_min_raise_and_6max_btn_postflop_position() {
    let hu_v = json!({
        "hand_id": 5001, "phase": "Preflop", "pot": 300, "current_bet": 200, "big_blind": 100, "small_blind": 50, "active_player_id": "hero", "board": [],
        "players": [{ "id": "hero", "name": "Hero", "chips": 9900, "bet": 100, "is_small_blind": false, "hole": ["As", "Kd"] }, { "id": "villain", "name": "Villain", "chips": 9800, "bet": 200, "is_small_blind": true }]
    });
    let snap = parse_live_state(&hu_v, "hero").expect("valid HU snapshot");
    let (mut rt, mut tracker) = (LiveRoundTracker::new(), OpponentTracker::new());
    rt.infer_and_record_opponent_action(&snap, &mut tracker);
    assert_eq!(rt.live_history[0].as_slice(), &[RAISE_MIN]);
    assert_eq!(tracker.pfr_hands, 1);

    let sixmax_btn_flop = json!({
        "hand_id": 5002, "phase": "Flop", "pot": 600, "current_bet": 0, "big_blind": 100, "small_blind": 50, "active_player_id": "hero", "board": ["As", "7d", "2c"],
        "players": [
            { "id": "hero", "name": "Hero", "chips": 9700, "bet": 0, "folded": false, "hole": ["Kh", "Qh"] },
            { "id": "sb", "name": "SB", "chips": 9700, "bet": 0, "is_small_blind": true, "folded": false },
            { "id": "bb", "name": "BB", "chips": 9900, "bet": 0, "is_big_blind": true, "folded": true },
            { "id": "utg", "name": "UTG", "chips": 10000, "bet": 0, "folded": true }
        ]
    });
    let s6 = parse_live_state(&sixmax_btn_flop, "hero").expect("valid 6-max");
    assert_eq!(s6.my_player, 0);
}

#[test]
fn test_postflop_overbet_bluffer_detection() {
    let (mut rt, mut tracker) = (LiveRoundTracker::new(), OpponentTracker::new());
    observe_hu_n(
        &mut rt,
        &mut tracker,
        1..=4,
        json!({ "phase": "Flop", "pot": 1875, "current_bet": 1125, "big_blind": 150, "small_blind": 75, "active_player_id": "hero", "board": ["3d", "9c", "7h"], "players": [{ "id": "hero", "name": "Hero", "chips": 8000, "bet": 0, "is_small_blind": true, "hole": ["Ks", "7d"] }, { "id": "villain", "name": "Villain", "chips": 8000, "bet": 1125, "is_small_blind": false }] }),
    );
    assert_eq!(tracker.postflop_overbets, 4);
    assert!(tracker.is_overbet_bluffer());

    let (mut rt_r, mut spammer) = (LiveRoundTracker::new(), OpponentTracker::new());
    observe_hu_n(
        &mut rt_r,
        &mut spammer,
        101..=110,
        json!({ "phase": "Flop", "pot": 1500, "current_bet": 500, "big_blind": 200, "small_blind": 100, "active_player_id": "hero", "board": ["Js", "5d", "2c"], "players": [{ "id": "hero", "name": "Hero", "chips": 9000, "bet": 0, "is_small_blind": true, "hole": ["Kh", "Td"] }, { "id": "villain", "name": "Villain", "chips": 8500, "bet": 500, "is_small_blind": false }] }),
    );
    assert!(!spammer.is_overbet_bluffer());

    for (id, pot, bet, hc) in [(201u64, 5400, 3400, 5000), (202u64, 18039, 15639, 1961)] {
        observe_hu_n(
            &mut rt_r,
            &mut spammer,
            id..=id,
            json!({ "phase": "River", "pot": pot, "current_bet": bet, "big_blind": 300, "small_blind": 150, "active_player_id": "hero", "board": ["Jh", "5s", "8s", "2d", "Qc"], "players": [{ "id": "hero", "name": "Hero", "chips": hc, "bet": 0, "is_small_blind": false, "hole": ["8d", "6h"] }, { "id": "villain", "name": "Villain", "chips": 0, "bet": bet, "is_small_blind": true }] }),
        );
    }
    assert_eq!(spammer.river_overbets, 2);
    assert!(spammer.is_overbet_bluffer());
}

#[test]
fn test_effective_remaining_chips_and_snapshot_opp_stack_parsing() {
    assert_eq!(effective_remaining_chips(18000, None, 2000), 18000);
    assert_eq!(effective_remaining_chips(18000, Some(0), 2000), 2000);
    assert_eq!(effective_remaining_chips(15000, Some(2000), 2000), 4000);
    assert_eq!(effective_remaining_chips(1500, Some(2000), 2000), 1500);

    for (payload, expected_opp_chips, exp_eff) in [
        (
            json!({ "hand_id": 6001, "phase": "River", "pot": 4000, "current_bet": 2000, "big_blind": 200, "small_blind": 100, "active_player_id": "hero", "board": ["As", "Kd", "6c", "6h", "2d"], "players": [{ "id": "hero", "name": "Hero", "chips": 18000, "bet": 0, "is_small_blind": true, "hole": ["Js", "Jh"] }, { "id": "opp", "name": "Villain", "chips": 0, "bet": 2000, "is_small_blind": false }] }),
            Some(0),
            2000,
        ),
        (
            json!({ "hand_id": 6002, "phase": "Flop", "pot": 600, "current_bet": 200, "big_blind": 100, "small_blind": 50, "active_player_id": "hero", "board": ["As", "Kd", "2c"], "players": [{ "id": "hero", "name": "Hero", "chips": 10000, "bet": 0, "hole": ["Ah", "Qd"] }, { "id": "opp", "name": "Villain", "bet": 200 }] }),
            None,
            10000,
        ),
        (
            json!({ "hand_id": 6003, "phase": "Turn", "pot": 2200, "current_bet": 1000, "big_blind": 100, "small_blind": 50, "active_player_id": "hero", "board": ["As", "Kd", "6c", "2h"], "players": [{ "id": "hero", "name": "Hero", "chips": 10000, "bet": 0, "folded": false, "hole": ["Ah", "Kc"] }, { "id": "short_allin", "name": "Short", "chips": 0, "bet": 1000, "folded": false }, { "id": "folded_opp", "name": "Folded", "chips": 20000, "bet": 0, "folded": true }, { "id": "deep_opp", "name": "Deep", "chips": 10000, "bet": 200, "folded": false }] }),
            Some(9200),
            10000,
        ),
    ] {
        let s = parse_live_state(&payload, "hero").expect("parse opp stack");
        assert_eq!(s.opp_chips, expected_opp_chips);
        let eff = effective_remaining_chips(s.my_chips, s.opp_chips, s.to_call);
        assert_eq!(eff, exp_eff);
    }
}

#[test]
fn test_emergency_fallback_candidate_order_and_safety() {
    let dl = legal_list(&["fold", "call", "raise", "all_in", "allin"]);
    let r1 = cands(&dl, None, "invalid call amount", 200, 10000);
    assert_eq!(r1, vec![("call", 200)]);

    let sb = cands(&dl, Some(("call", 50)), "rejected", 50, 10000);
    assert!(sb.iter().all(|(a, _)| *a != "all_in" && *a != "allin"));

    let oc = legal_list(&["fold", "call", "all_in"]);
    let r2 = cands(&oc, None, "amount mismatch", 1, 1);
    assert_eq!(r2, vec![("call", 1), ("all_in", 0)]);
    let r3 = cands(&oc, Some(("call", 1)), "amount mismatch", 1, 1);
    assert_eq!(r3, vec![("all_in", 0)]);
}

#[test]
fn test_parse_live_state_prefers_participant_id_for_profile_key() {
    let payload = json!({
        "phase": "Preflop", "pot": 150, "current_bet": 100, "big_blind": 100, "active_player_id": "hero",
        "players": [
            { "id": "hero", "name": "RustBot", "hole": ["As", "Kd"], "bet": 50, "chips": 9950, "is_small_blind": true },
            { "id": "seat_2", "participant_id": "bot_uuid_42", "name": "RotatingName", "bet": 100, "chips": 9900, "is_big_blind": true }
        ],
        "legal_actions": ["fold", "call", "raise"]
    });
    let s = parse_live_state(&payload, "hero").expect("pid");
    assert_eq!(s.opp_name, "bot_uuid_42");
}
