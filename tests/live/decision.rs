use super::{board, hole};
use lil_poker_mccfr::cfr::abstraction::{
    nash_open_push_max_bb, postflop_equity_bucket, preflop_bucket,
};
use lil_poker_mccfr::cfr::fallback::StrategyLookupIndex;
use lil_poker_mccfr::cfr::opponent_model::{DecisionTrace, GuardId, OpponentTracker};
use lil_poker_mccfr::cfr::subgame::{SubgameContext, SubgameSolver};
use lil_poker_mccfr::game::config::{RAISE_HALF_POT, RAISE_THIRD_POT};
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, Card, FOLD, NUM_ROUNDS, RAISE_MIN, RoundHistory,
};
use lil_poker_mccfr::live::decision::{LiveDecisionRequest, decide_action, decide_action_traced};
use lil_poker_mccfr::live::protocol::ActionTranslationBlend;
use lil_poker_mccfr::live::state::derive_legal_actions;
use std::collections::HashMap;

fn legal_list(acts: &[&str]) -> Vec<String> {
    acts.iter().map(|s| (*s).to_string()).collect()
}

fn base_req<'a>(
    hole: [Card; 2],
    board: &'a [Card],
    legal: &'a [String],
    pot: i32,
    to_call: i32,
) -> LiveDecisionRequest<'a> {
    LiveDecisionRequest {
        hole,
        board,
        legal,
        pot,
        to_call,
        history: [RoundHistory::new(); NUM_ROUNDS],
        ..Default::default()
    }
}

fn p1_req<'a>(
    base: LiveDecisionRequest<'a>,
    contrib: [i32; 2],
    state: (i32, i32, u8, i32),
) -> LiveDecisionRequest<'a> {
    LiveDecisionRequest {
        my_player: 1,
        contributions: contrib,
        big_blind: state.0,
        my_chips: state.1,
        raises_this_round: state.2,
        current_bet: state.3,
        ..base
    }
}

fn empty_decide(req: &LiveDecisionRequest<'_>, tracker: &OpponentTracker) -> (String, i32) {
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    decide_action(req, &strategy_map, tracker, None)
}

fn trace_empty(
    req: &LiveDecisionRequest<'_>,
    tracker: &OpponentTracker,
) -> (String, i32, DecisionTrace) {
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    decide_action_traced(req, &strategy_map, tracker, None)
}

#[test]
fn test_live_decide_action_with_real_contributions() {
    let (h, b) = (hole("As Ks"), board("Qs Jh Td"));
    let tracker = OpponentTracker::new();
    let strategy_map: HashMap<String, Vec<f64>> = HashMap::new();
    let index = StrategyLookupIndex::new();
    let solver = SubgameSolver::new(50);
    let mut ctx = SubgameContext::new();

    let legal_chk = legal_list(&["fold", "check", "bet", "allin"]);
    let mut r1 = base_req(h, &b, &legal_chk, 200, 0);
    r1.subgame_search = true;
    r1.contributions = [100, 100];
    r1.strategy_index = Some(&index);
    r1.solver = Some(&solver);
    let (act1, amt1) = decide_action(&r1, &strategy_map, &tracker, Some(&mut ctx));
    assert!(!act1.is_empty() && act1 != "fold" && amt1 >= 0);

    let legal_raise = legal_list(&["fold", "call", "raise", "allin"]);
    let mut r2 = p1_req(
        base_req(h, &b, &legal_raise, 350, 150),
        [250, 100],
        (100, 10_000, 1, 150),
    );
    r2.subgame_search = true;
    r2.strategy_index = Some(&index);
    r2.solver = Some(&solver);
    let (act2, amt2) = decide_action(&r2, &strategy_map, &tracker, Some(&mut ctx));
    assert!(!act2.is_empty() && act2 != "fold" && amt2 >= 0);
}

#[test]
fn test_bot_value_bets_straight_in_live_game() {
    let (h, b) = (hole("Ts 8d"), board("Qd Tc Js 9d"));
    let legal = derive_legal_actions(0, 9600);
    let tracker = OpponentTracker::new();
    let solver = SubgameSolver::new(400);
    let mut ctx = SubgameContext::new();
    let empty_map: HashMap<String, [f64; 6]> = HashMap::new();
    let index = StrategyLookupIndex::new();
    let mut req = p1_req(
        base_req(h, &b, &legal, 200, 0),
        [100, 100],
        (100, 10_000, 0, 0),
    );
    req.subgame_search = true;
    req.strategy_index = Some(&index);
    req.solver = Some(&solver);

    let (mut raise_count, num_trials) = (0usize, 60usize);
    for _ in 0..num_trials {
        let (action, amount) = decide_action(&req, &empty_map, &tracker, Some(&mut ctx));
        if action == "raise" {
            raise_count += 1;
            assert!(amount >= 100, "Raise amount must be at least min_raise");
        } else if action == "all_in" || action == "allin" {
            raise_count += 1;
        }
    }
    let raise_rate = raise_count as f64 / num_trials as f64;
    let pct = raise_rate * 100.0;
    assert!(
        raise_rate >= 0.50,
        "Straight value bet >= 50%, got {pct:.1}% ({raise_count}/{num_trials})"
    );
}

#[test]
fn test_short_stack_bb_check_and_min_raise_defense() {
    let tracker = OpponentTracker::new();
    let legal_bb = legal_list(&["fold", "check", "raise", "allin"]);
    let legal_pf = legal_list(&["fold", "call", "raise", "allin"]);

    let mut r1 = p1_req(
        base_req(hole("9h 6h"), &[], &legal_bb, 400, 0),
        [200, 200],
        (200, 2200, 0, 0),
    );
    r1.subgame_search = true;
    assert_eq!(
        empty_decide(&r1, &tracker).0,
        "check",
        "9h6h BB limp must check"
    );

    let mut r2 = p1_req(
        base_req(hole("Ah 4c"), &[], &legal_pf, 600, 200),
        [200, 400],
        (200, 1950, 0, 0),
    );
    r2.subgame_search = true;
    assert_eq!(
        empty_decide(&r2, &tracker).0,
        "call",
        "A4o at 10 BB facing 2x should flat-call"
    );
}

#[test]
fn test_6max_small_pair_folds_8x_open_and_flop_shove() {
    let tracker = OpponentTracker::new();
    let hole_66 = hole("6h 6d");
    let legal_pf = legal_list(&["fold", "call", "raise", "allin"]);
    let legal_shove = legal_list(&["fold", "call", "allin"]);

    let mut r1 = p1_req(
        base_req(hole_66, &[], &legal_pf, 3800, 3200),
        [200, 3200],
        (400, 12000, 1, 3200),
    );
    r1.active_opponents = 2;
    assert_eq!(
        trace_empty(&r1, &tracker).0,
        "fold",
        "66 must fold vs 8x open"
    );

    let board_j82 = board("Jc 8s 2d");
    let r2 = p1_req(
        base_req(hole_66, &board_j82, &legal_shove, 15200, 8000),
        [3200, 3200],
        (400, 8000, 1, 8000),
    );
    assert_eq!(
        trace_empty(&r2, &tracker).0,
        "fold",
        "66 on J-8-2 must fold vs shove"
    );
}

#[test]
fn test_hu_bare_flush_draw_flop_raise_and_3bet_discipline() {
    let tracker = OpponentTracker::new();
    let (hole_j6s, board_583) = (hole("Js 6s"), board("5s 8c 3s"));
    let legal = legal_list(&["fold", "call", "raise", "allin"]);

    let r1 = p1_req(
        base_req(hole_j6s, &board_583, &legal, 1200, 600),
        [300, 300],
        (150, 9000, 0, 600),
    );
    let act_vs_bet = trace_empty(&r1, &tracker).0;
    assert_ne!(
        act_vs_bet, "raise",
        "Bare draw should not check-raise vs large flop bet"
    );
    assert_ne!(
        act_vs_bet, "allin",
        "Bare draw should not shove vs flop bet"
    );

    let r2 = p1_req(
        base_req(hole_j6s, &board_583, &legal, 7500, 3750),
        [300, 300],
        (150, 8000, 1, 3750),
    );
    assert_eq!(
        trace_empty(&r2, &tracker).0,
        "fold",
        "Bare FD must fold facing 25 BB 3-bet"
    );
}

#[test]
fn test_hu_suited_connector_12bb_no_open_jam() {
    let hole_76s = hole("7h 6h");
    let max_bb = nash_open_push_max_bb(preflop_bucket(hole_76s[0], hole_76s[1]).0, 1);
    assert!(
        max_bb <= 9,
        "76s push threshold should be <= 9 BB (got {max_bb})"
    );

    let legal = legal_list(&["fold", "call", "raise", "allin"]);
    let mut req = base_req(hole_76s, &[], &legal, 1200, 400);
    req.contributions = [400, 800];
    req.big_blind = 800;
    req.my_chips = 10000;
    req.current_bet = 800;
    assert_ne!(
        trace_empty(&req, &OpponentTracker::new()).0,
        "allin",
        "76s at 12.5 BB must not open-jam all-in"
    );
}

#[test]
fn test_33bb_aqo_folds_vs_tag_shove_and_calls_maniac() {
    let hole_aqo = hole("Ac Qd");
    let legal = legal_list(&["fold", "call", "allin"]);
    let shove_req = |chips: i32, pot: i32| {
        p1_req(
            base_req(hole_aqo, &[], &legal, pot, chips),
            [200, 200],
            (200, chips, 1, chips),
        )
    };

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
    assert_eq!(trace_empty(&shove_req(6600, 7000), &tag_tracker).0, "fold");

    let hu_28_calls = (0..30)
        .filter(|_| trace_empty(&shove_req(5600, 6000), &tag_tracker).0 != "fold")
        .count();
    assert!(
        hu_28_calls >= 12,
        "AQo should call <= 30 BB HU shove ({hu_28_calls}/30)"
    );

    let mut maniac_tracker = OpponentTracker::new();
    for _ in 0..16 {
        maniac_tracker.record_action_street_detailed(ALL_IN, 1, false, 4000, 600);
        maniac_tracker.record_preflop_positional(true, true, true);
        maniac_tracker.end_hand();
    }
    assert_ne!(
        trace_empty(&shove_req(6600, 7000), &maniac_tracker).0,
        "fold"
    );
}

#[test]
fn test_preflop_micro_stack_1chip_calls() {
    let legal_1chip = legal_list(&["fold", "call", "allin"]);
    let req = p1_req(
        base_req(hole("Kd Qc"), &[], &legal_1chip, 5248, 1),
        [2624, 2624],
        (400, 1, 1, 13124),
    );
    let (act_1chip, _, tr_1chip) = trace_empty(&req, &OpponentTracker::new());
    let reasons = &tr_1chip.guard_reasons;
    assert_eq!(
        act_1chip, "call",
        "1 chip call in 5248 pot (reasons: {reasons:?})"
    );
}

#[test]
fn test_flop_oop_donk_suppresses_min_and_third_pot() {
    let tracker = OpponentTracker::new();
    let (h, b) = (hole("Ah 6h"), board("Kd Js 8c"));
    let bucket = postflop_equity_bucket(&h, &b);
    let key = format!("P:rc|F:B{bucket:02}/");
    let legal = legal_list(&["fold", "check", "bet", "allin"]);
    let mut req = p1_req(
        base_req(h, &b, &legal, 800, 0),
        [400, 400],
        (200, 4400, 0, 0),
    );
    req.history[0].push(RAISE_MIN);
    req.history[0].push(CALL_CHECK);
    req.opp_chips = Some(15000);

    for probs in [
        vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    ] {
        let mut strategy_map = HashMap::new();
        strategy_map.insert(key.clone(), probs);
        let (act, amt, tr) = decide_action_traced(&req, &strategy_map, &tracker, None);
        let reasons = &tr.guard_reasons;
        assert_eq!(
            act, "check",
            "OOP flop donk bucket {bucket} (reasons: {reasons:?})"
        );
        assert_eq!(amt, 0);
        assert!(tr.guard_reasons.contains(GuardId::FlopOopMicroDonkSuppress));
    }
}

#[test]
fn test_live_decision_pseudo_harmonic_linear_interpolation() {
    let mut tracker = OpponentTracker::new();
    tracker.set_exploit_enabled(false);
    let (h, b) = (hole("Ah Kh"), board("As 8c 4d"));
    let bucket = postflop_equity_bucket(&h, &b);

    let mut strategy_map = HashMap::new();
    strategy_map.insert(
        format!("P:rc|F:B{bucket:02}/ct"),
        vec![0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
    );
    strategy_map.insert(
        format!("P:rc|F:B{bucket:02}/ch"),
        vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    );

    let legal = legal_list(&["fold", "call"]);
    let mut req = base_req(h, &b, &legal, 500, 250);
    req.history[0].push(RAISE_MIN);
    req.history[0].push(CALL_CHECK);
    req.history[1].push(CALL_CHECK);
    req.history[1].push(RAISE_THIRD_POT);
    req.my_player = 0;
    req.contributions = [250, 500];
    req.big_blind = 100;
    req.my_chips = 9500;
    req.opp_chips = Some(9500);

    for (low_weight, expected_top, expected_act) in [
        (0.85, Some(CALL_CHECK), None),
        (0.10, Some(FOLD), None),
        (0.99, None, Some("call")),
        (0.01, None, Some("fold")),
    ] {
        req.active_bet_blend = Some(ActionTranslationBlend {
            low_action: RAISE_THIRD_POT,
            high_action: RAISE_HALF_POT,
            low_weight,
        });
        let (act, _, tr) = decide_action_traced(&req, &strategy_map, &tracker, None);
        if let Some(top) = expected_top {
            assert_eq!(tr.raw_top_action, top, "low_weight {low_weight}");
        }
        if let Some(exp_act) = expected_act {
            assert_eq!(act, exp_act, "low_weight {low_weight}");
        }
    }
}
