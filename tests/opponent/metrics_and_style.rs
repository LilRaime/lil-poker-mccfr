use super::{LEGAL_FULL, LEGAL_OPEN, board, hole, make_passive_station};
use lil_poker_mccfr::cfr::abstraction::PostflopBucket;
use lil_poker_mccfr::cfr::opponent_model::{
    ExploitRequest, GuardId, GuardSet, OpponentStyle, OpponentTracker, PolicyRequest,
};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, RoundHistory,
};
use lil_poker_mccfr::sim::OpponentArchetype;
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[test]
fn test_opponent_tracker_advanced_metrics() {
    let mut tracker = OpponentTracker::new();
    /* First sequence: Preflop open raise, Flop c-bet faced and folded */
    tracker.record_action_street(RAISE_MIN, 1, false);
    tracker.record_action_street(FOLD, 2, true);
    tracker.end_hand();
    assert_eq!(
        (
            tracker.total_hands,
            tracker.vpip_hands,
            tracker.pfr_hands,
            tracker.saw_flop,
            tracker.flop_cbet_faced,
            tracker.flop_fold_to_cbet
        ),
        (1, 1, 1, 1, 1, 1)
    );

    /* Second sequence: Preflop limp/call, Flop call c-bet, Showdown reached */
    for (act, round, facing) in [
        (CALL_CHECK, 1, false),
        (CALL_CHECK, 2, true),
        (CALL_CHECK, 3, false),
        (CALL_CHECK, 4, false),
    ] {
        tracker.record_action_street(act, round, facing);
    }
    tracker.record_showdown();
    tracker.end_hand();
    assert_eq!(
        (
            tracker.total_hands,
            tracker.vpip_hands,
            tracker.pfr_hands,
            tracker.flop_cbet_faced,
            tracker.flop_fold_to_cbet,
            tracker.went_to_showdown
        ),
        (2, 2, 1, 2, 1, 1)
    );
}

#[test]
fn test_opponent_style_classification_and_showdown() {
    let mut tracker = make_passive_station();
    let (style, conf) = tracker.classify_style();
    assert_eq!(style, OpponentStyle::CallingStation);
    assert!(conf > 0.50);

    /* Record a showdown where opponent bluffed river with 7-2 air */
    tracker.record_showdown_hand(hole("7s 2c"), &board("Ad Kh Qs 9d 3c"), true);
    assert_eq!(
        (tracker.showdown_hands, tracker.showdown_bluff_count),
        (1, 1)
    );
    assert_eq!(tracker.showdown_bluff_rate(), 1.0);
}

#[test]
fn test_opponent_barrel_metrics_and_wilson() {
    let mut tracker = OpponentTracker::new();
    let (low, high) = OpponentTracker::wilson_confidence(50, 100, 1.96);
    assert!(
        (0.39..=0.42).contains(&low),
        "Wilson 95% lower bound for 50/100 should be ~0.40, got {low}"
    );
    assert!(
        (0.58..=0.61).contains(&high),
        "Wilson 95% upper bound for 50/100 should be ~0.60, got {high}"
    );

    for (act, round, facing) in [
        (RAISE_MIN, 1, false),
        (CALL_CHECK, 2, false),
        (FOLD, 3, true),
    ] {
        tracker.record_action_street(act, round, facing);
    }
    tracker.end_hand();
    assert_eq!((tracker.turn_cbet_faced, tracker.turn_fold_to_cbet), (1, 1));
    assert_eq!(tracker.turn_fold_to_cbet_ratio(), 1.0);

    for round in 1..=3 {
        tracker.record_action_street(CALL_CHECK, round, false);
    }
    tracker.record_action_street(FOLD, 4, true);
    tracker.end_hand();
    for _ in 0..4 {
        tracker.record_action_street(FOLD, 1, false);
        tracker.end_hand();
    }
    assert_eq!((tracker.river_bet_faced, tracker.river_fold_to_bet), (1, 1));
    assert_eq!(tracker.river_fold_to_bet_ratio(), 1.0);

    let base_strat = [0.30, 0.40, 0.10, 0.10, 0.10, 0.0];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT, RAISE_HALF_POT];
    let adj_turn = tracker.adjust_strategy(&ExploitRequest {
        blueprint_probs: &base_strat,
        legal_actions: &legal,
        bucket: 10,
        round: 3,
        ..Default::default()
    });
    let (orig_raise, adj_raise) = (base_strat[3] + base_strat[4], adj_turn[3] + adj_turn[4]);
    assert!(
        adj_raise > orig_raise,
        "Facing turn over-folder, bot should fire 2nd barrel bluff more often (orig {orig_raise}, adj {adj_raise})"
    );
}

#[test]
fn test_opponent_sizing_tells() {
    let mut tracker = OpponentTracker::new();
    for (act, round, bet, pot) in [
        (RAISE_MIN, 2, 20, 100),
        (RAISE_HALF_POT, 2, 50, 100),
        (ALL_IN, 3, 150, 100),
    ] {
        tracker.record_action_street_detailed(act, round, false, bet, pot);
    }
    assert_eq!(
        (
            tracker.sizing_small_bets,
            tracker.sizing_mid_bets,
            tracker.sizing_large_bets
        ),
        (1, 1, 1)
    );
    tracker.record_showdown_hand_detailed(hole("As Ac"), &board("Ah Ad Ks Qs 2d"), true, 800, 1000);
    assert_eq!(tracker.showdown_large_monster_count, 1);
}

#[test]
fn test_opponent_positional_and_3bet_tracking() {
    let mut tracker = OpponentTracker::new();
    for (is_sb, vpip, pfr, n) in [
        (true, true, true, 6),
        (true, true, false, 2),
        (true, false, false, 2),
        (false, true, false, 5),
        (false, false, false, 5),
    ] {
        for _ in 0..n {
            tracker.record_preflop_positional(is_sb, vpip, pfr);
        }
    }
    assert_eq!(
        (
            tracker.sb_total_hands,
            tracker.sb_vpip_hands,
            tracker.sb_pfr_hands
        ),
        (10, 8, 6)
    );
    assert!(
        (tracker.sb_vpip_ratio() - 0.80).abs() < 1e-6
            && (tracker.sb_pfr_ratio() - 0.60).abs() < 1e-6
    );
    assert_eq!((tracker.bb_total_hands, tracker.bb_vpip_hands), (10, 5));
    assert!((tracker.bb_vpip_ratio() - 0.50).abs() < 1e-6);

    for folded in [true, true, true, false] {
        tracker.record_facing_3bet(folded);
    }
    assert_eq!(
        (tracker.faced_3bet_count, tracker.fold_to_3bet_count),
        (4, 3)
    );
    assert!((tracker.fold_to_3bet_ratio() - 0.75).abs() < 1e-6);
}

#[test]
fn test_opponent_showdown_hand_tier_tracking() {
    let mut tracker = OpponentTracker::new();
    let b = board("Kh 8d 2c 4s 9d");

    /* 1. AKo (Tier 1) must NOT count as loose trash and MUST increment showdown_trap_count */
    tracker.record_showdown_hand_detailed(hole("As Kh"), &b, false, 0, 100);
    assert_eq!(
        tracker.showdown_loose_count, 0,
        "AKo is a Tier 1 hand and must NEVER increment showdown_loose_count"
    );
    assert_eq!(
        tracker.showdown_trap_count, 1,
        "AKo is a Tier 1 premium hand and should increment showdown_trap_count"
    );

    /* 2. 72o (Tier 5 pure trash) must increment showdown_loose_count */
    tracker.record_showdown_hand_detailed(hole("7d 2c"), &b, false, 0, 100);
    assert_eq!(
        tracker.showdown_loose_count, 1,
        "72o is Tier 5 trash and must increment showdown_loose_count"
    );
}

#[test]
fn test_showdown_trap_false_positive_filtering_and_exploit() {
    let mut tracker = OpponentTracker::new();
    let aa_hole = hole("Ah As");
    let b = board("7c 5d 2s Jh 4c");

    /* 1-2. AA in a 3-bet pot or with postflop aggression must NOT count as a slowplay trap */
    for (actions, pot, msg) in [
        (
            &[
                (RAISE_HALF_POT, 1, 300, 150),
                (RAISE_HALF_POT, 1, 900, 1200),
            ][..],
            1800,
            "AA in a 3-bet pot must not falsely increment showdown_trap_count",
        ),
        (
            &[(RAISE_THIRD_POT, 2, 400, 400)][..],
            1200,
            "AA with postflop aggression must not falsely increment showdown_trap_count",
        ),
    ] {
        for &(act, round, bet, p) in actions {
            tracker.record_action_street_detailed(act, round, false, bet, p);
        }
        tracker.record_showdown_hand_detailed(aa_hole, &b, false, 0, pot);
        tracker.end_hand();
        assert_eq!(tracker.showdown_trap_count, 0, "{msg}");
    }

    /* 3. Passive AA & KK DOES count (showdown_trap_count == 2, ratio = 2/4 = 0.50 >= 0.25) */
    for (h, with_flop, expected) in [(aa_hole, true, 1), (hole("Kh Kc"), false, 2)] {
        tracker.record_action_street_detailed(CALL_CHECK, 1, false, 100, 150);
        if with_flop {
            tracker.record_action_street_detailed(CALL_CHECK, 2, false, 0, 200);
        }
        tracker.record_showdown_hand_detailed(h, &b, false, 0, 200);
        tracker.end_hand();
        assert_eq!(tracker.showdown_trap_count, expected);
    }

    let base: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.40, 0.10, 0.10, 0.30, 0.10];
    let adj = tracker.adjust_strategy(&ExploitRequest {
        blueprint_probs: &base,
        legal_actions: &LEGAL_FULL,
        bucket: 32,
        round: 4,
        to_call: 0,
        table_bb: 1000,
    });
    assert!(
        adj[CALL_CHECK as usize] > base[CALL_CHECK as usize]
            && adj[RAISE_HALF_POT as usize] < base[RAISE_HALF_POT as usize],
        "Against genuine showdown trapper, River check-back must rise ({:.3} -> {:.3}) and big bet drop ({:.3} -> {:.3})",
        base[CALL_CHECK as usize],
        adj[CALL_CHECK as usize],
        base[RAISE_HALF_POT as usize],
        adj[RAISE_HALF_POT as usize]
    );
}

#[test]
fn test_observe_blind_level_decay() {
    let mut tracker = OpponentTracker::new();
    tracker.observe_blind_level(100);
    for i in 0..20 {
        if i < 10 {
            tracker.record_action_street_detailed(RAISE_MIN, 1, false, 200, 150);
        } else {
            tracker.record_action_street_detailed(FOLD, 1, true, 0, 250);
        }
        tracker.end_hand();
    }
    assert_eq!((tracker.total_hands, tracker.pfr_hands), (20, 10));
    tracker.observe_blind_level(100);
    assert_eq!(tracker.total_hands, 20);
    tracker.observe_blind_level(200);
    let total = tracker.total_hands;
    assert!(
        (12..20).contains(&total),
        "Expected soft decay on 2x blind jump (100 -> 200), got total_hands={total}"
    );
    assert!(tracker.pfr_hands <= tracker.vpip_hands && tracker.vpip_hands <= tracker.total_hands);
}

#[test]
fn test_opponent_profile_save_and_load() {
    let mut persisted = OpponentTracker::new();
    for _ in 0..10 {
        persisted.record_action_street_detailed(CALL_CHECK, 1, false, 100, 200);
        persisted.record_action_street_detailed(CALL_CHECK, 1, false, 200, 400);
        persisted.end_hand();
    }
    assert_eq!((persisted.total_hands, persisted.vpip_hands), (10, 10));
    assert!((persisted.vpip_ratio() - 1.0).abs() < 1e-6);

    let tmp_path = "/tmp/lil_poker_test_opp_profiles.json";
    persisted
        .save_profile(tmp_path, "test_villain")
        .expect("save profile");
    let loaded =
        OpponentTracker::load_profile(tmp_path, "test_villain").expect("load saved profile");
    assert_eq!(loaded.total_hands, 9);
    let _ = std::fs::remove_file(tmp_path);
}

#[test]
fn test_showdown_thin_value_pair_not_counted_as_bluff_and_street_profile() {
    let mut tracker = OpponentTracker::new();
    let b = board("Kh 8d 4c 2s 9c");

    /* 1. River raise with AA (Overpair) counts as thin value pair, NEVER bluff */
    tracker.record_showdown_hand_detailed(hole("As Ac"), &b, true, 1400, 1000);
    assert_eq!(
        (
            tracker.showdown_bluff_count,
            tracker.showdown_thin_value_pair_count,
            tracker.showdown_overbet_hands,
            tracker.showdown_overbet_value_hands
        ),
        (0, 1, 1, 1)
    );

    /* 2. River raise with QJ air increments showdown_bluff_count */
    tracker.record_showdown_hand_detailed(hole("Qs Jd"), &b, true, 1500, 1000);
    assert_eq!(
        (
            tracker.showdown_bluff_count,
            tracker.showdown_thin_value_pair_count,
            tracker.showdown_overbet_hands
        ),
        (1, 1, 2)
    );
    assert!((tracker.overbet_value_rate() - 0.50).abs() < 1e-6);

    /* 3. Flop C-Bet and Turn Barrel frequency + Subgame street profile population */
    for _ in 0..4 {
        tracker.record_action_street_detailed(RAISE_MIN, 1, false, 250, 150);
        tracker.record_action_street_detailed(RAISE_THIRD_POT, 2, false, 300, 500);
        tracker.record_action_street_detailed(CALL_CHECK, 3, false, 0, 1100);
        tracker.record_action_street_detailed(FOLD, 3, true, 0, 1100);
        tracker.end_hand();
    }
    for _ in 0..3 {
        tracker.record_action_street_detailed(FOLD, 4, true, 0, 1000);
        tracker.end_hand();
    }
    assert!(
        (tracker.flop_cbet_freq() - 1.0).abs() < 1e-6
            && (tracker.turn_barrel_freq() - 0.0).abs() < 1e-6
    );
    let prof = tracker
        .subgame_street_profile()
        .expect("street profile should populate after >= 3 facing samples");
    assert!(
        prof.turn_fold_to_barrel.unwrap_or(0.0) > 0.65
            && prof.river_fold_to_bet.unwrap_or(0.0) > 0.55
    );

    tracker.apply_session_decay(0.50);
    assert!(
        tracker.flop_cbet_made <= tracker.flop_cbet_opps
            && tracker.turn_barrel_made <= tracker.turn_barrel_opps
            && tracker.showdown_overbet_value_hands <= tracker.showdown_overbet_hands
    );
}

#[test]
fn test_check_tracking_tactical_profile_and_unified_post_selection() {
    let mut tracker = OpponentTracker::new();
    for _ in 0..4 {
        tracker.record_action_street_detailed(CALL_CHECK, 1, true, 150, 300);
        for round in 2..=4 {
            tracker.record_action_street_detailed(CALL_CHECK, round, false, 0, 400);
        }
        tracker.end_hand();
    }
    assert_eq!(
        (
            tracker.check_count,
            tracker.flop_checks,
            tracker.turn_checks,
            tracker.river_checks,
            tracker.genuine_call_count()
        ),
        (12, 4, 4, 4, 4)
    );
    assert!((tracker.check_ratio() - 0.75).abs() < 1e-6);
    assert!(tracker.afq_ratio() < 1e-6 && tracker.postflop_afq() < 1e-6);

    tracker.apply_session_decay(0.50);
    assert!(tracker.check_count <= tracker.call_count);

    let mut station = make_passive_station();
    for _ in 0..8 {
        station.record_preflop_positional(false, true, false);
    }
    let profile = station.tactical_profile();
    assert!(profile.is_passive_station && profile.is_sticky_caller && profile.is_ultra_sticky_bb);

    let mut rng = SmallRng::seed_from_u64(20_261_126);
    let mut hist = [RoundHistory::new(); 4];
    hist[0].push(RAISE_MIN);
    hist[0].push(CALL_CHECK);
    let donk_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.15, 0.45, 0.25, 0.15, 0.0];
    let (act, tr) = OpponentTracker::new().select_action_traced(
        &PolicyRequest {
            raw_probs: &donk_heavy,
            legal_actions: &LEGAL_OPEN,
            bucket: 32,
            round: 2,
            to_call: 0,
            pot: 600,
            my_player: Some(1),
            history: Some(&hist),
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(act, CALL_CHECK);
    assert!(tr.guard_reasons.contains(GuardId::FlopOopMicroDonkSuppress));
}

#[test]
fn test_zero_cost_guard_set_postflop_bucket_and_archetype_parsing() {
    for id in GuardId::ALL {
        assert_eq!(
            id.as_str()
                .parse::<GuardId>()
                .expect("valid canonical GuardId"),
            id
        );
    }
    let mut mask = GuardSet::EMPTY;
    assert!(mask.is_empty());
    mask.insert(GuardId::ShoveDefenseFold);
    mask.insert(GuardId::PostBarrelFold);
    assert_eq!(mask.len(), 2);
    assert!(mask.contains(GuardId::ShoveDefenseFold) && !mask.contains(GuardId::PfButtonSteal));
    assert_eq!(
        mask.into_iter().collect::<Vec<_>>(),
        vec![GuardId::ShoveDefenseFold, GuardId::PostBarrelFold]
    );

    let parsed_spec = GuardSet::from_disable_spec("shove_defense_fold, POST_BARREL_FOLD")
        .expect("comma-separated guard spec parses");
    assert_eq!(parsed_spec, mask);
    assert!(GuardSet::from_disable_spec("UNKNOWN_GUARD").is_err());

    let mut tracker = OpponentTracker::new();
    tracker.set_disabled_guards(["SHOVE_DEFENSE_FOLD"]);
    assert!(
        !tracker.is_guard_enabled(GuardId::ShoveDefenseFold)
            && tracker.is_guard_enabled(GuardId::PostBarrelFold)
    );
    tracker.set_disabled_guards(["ALL"]);
    assert!(!tracker.is_guard_enabled(GuardId::PfDisciplineGuard));

    assert!(PostflopBucket(33).has_pair_redraw() && PostflopBucket(38).has_pair_redraw());
    assert!(
        !PostflopBucket(33).is_bare_weak_or_mid_pair()
            && PostflopBucket(32).is_bare_weak_or_mid_pair()
    );
    assert!(
        PostflopBucket(36).is_clean_mid_pair()
            && PostflopBucket(73).is_board_chop_or_idiot_straight()
    );
    assert!(
        PostflopBucket(25).is_multiway_nut_draw() && !PostflopBucket(24).is_multiway_nut_draw()
    );

    for (raw, expected) in [
        ("fish", Some(OpponentArchetype::CallingStation)),
        ("nit", Some(OpponentArchetype::Rock)),
        ("model", Some(OpponentArchetype::SelfPlay)),
        ("callng_station", None),
    ] {
        assert_eq!(raw.parse::<OpponentArchetype>().ok(), expected);
    }

    /* Loose-passive postflop opponent with moderate preflop raises triggers is_passive_station() */
    let mut post_station = OpponentTracker::new();
    for i in 0..10 {
        if i < 2 {
            post_station.record_action_street_detailed(RAISE_MIN, 1, false, 200, 150);
        } else {
            post_station.record_action_street_detailed(CALL_CHECK, 1, true, 100, 200);
        }
        post_station.record_action_street_detailed(CALL_CHECK, 2, false, 0, 400);
        post_station.record_action_street_detailed(CALL_CHECK, 3, true, 200, 400);
        post_station.end_hand();
    }
    assert!(
        post_station.is_passive_station(),
        "High-VPIP loose-passive postflop opponent must trigger is_passive_station() despite occasional preflop raises"
    );
}
