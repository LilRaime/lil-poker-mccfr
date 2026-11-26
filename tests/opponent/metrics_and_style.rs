use super::{LEGAL_FULL, LEGAL_OPEN, make_passive_station};
use lil_poker_mccfr::cfr::abstraction::PostflopBucket;
use lil_poker_mccfr::cfr::opponent_model::{
    ExploitRequest, GuardId, GuardSet, OpponentStyle, OpponentTracker, PolicyRequest,
};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, Card, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, Rank, RoundHistory,
    Suit,
};
use lil_poker_mccfr::sim::OpponentArchetype;
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[test]
fn test_opponent_tracker_advanced_metrics() {
    let mut tracker = OpponentTracker::new();

    /* First sequence: Preflop open raise, Flop c-bet faced and folded */
    tracker.record_action_street(RAISE_MIN, 1, false);
    tracker.record_action_street(FOLD, 2, true); /* fold to c-bet */
    tracker.end_hand();

    assert_eq!(tracker.total_hands, 1);
    assert_eq!(tracker.vpip_hands, 1);
    assert_eq!(tracker.pfr_hands, 1);
    assert_eq!(tracker.saw_flop, 1);
    assert_eq!(tracker.flop_cbet_faced, 1);
    assert_eq!(tracker.flop_fold_to_cbet, 1);

    /* Second sequence: Preflop limp/call, Flop call c-bet, Showdown reached */
    tracker.record_action_street(CALL_CHECK, 1, false);
    tracker.record_action_street(CALL_CHECK, 2, true); /* call c-bet */
    tracker.record_action_street(CALL_CHECK, 3, false);
    tracker.record_action_street(CALL_CHECK, 4, false);
    tracker.record_showdown();
    tracker.end_hand();

    assert_eq!(tracker.total_hands, 2);
    assert_eq!(tracker.vpip_hands, 2);
    assert_eq!(tracker.pfr_hands, 1); /* was limp, so pfr unchanged */
    assert_eq!(tracker.flop_cbet_faced, 2);
    assert_eq!(tracker.flop_fold_to_cbet, 1);
    assert_eq!(tracker.went_to_showdown, 1);
}

#[test]
fn test_opponent_style_classification_and_showdown() {
    let mut tracker = make_passive_station();

    let (style, conf) = tracker.classify_style();
    assert_eq!(style, OpponentStyle::CallingStation);
    assert!(conf > 0.50);

    /* Record a showdown where opponent bluffed river with 7-2 air */
    let card = |rank, suit| Card { rank, suit };
    let opp_trash = [
        card(Rank::Seven, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];
    let board = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Spades),
        card(Rank::Nine, Suit::Diamonds),
        card(Rank::Three, Suit::Clubs),
    ];
    tracker.record_showdown_hand(opp_trash, &board, true);

    assert_eq!(tracker.showdown_hands, 1);
    assert_eq!(tracker.showdown_bluff_count, 1);
    assert_eq!(tracker.showdown_bluff_rate(), 1.0);
}

#[test]
fn test_opponent_barrel_metrics_and_wilson() {
    let mut tracker = OpponentTracker::new();

    /* Wilson score interval bounds checking */
    let (low, high) = OpponentTracker::wilson_confidence(50, 100, 1.96);
    assert!(
        (0.39..=0.42).contains(&low),
        "Wilson 95% lower bound for 50/100 should be ~0.40, got {low}"
    );
    assert!(
        (0.58..=0.61).contains(&high),
        "Wilson 95% upper bound for 50/100 should be ~0.60, got {high}"
    );

    /* Turn barrel facing and folding */
    tracker.record_action_street(RAISE_MIN, 1, false);
    tracker.record_action_street(CALL_CHECK, 2, false);
    tracker.record_action_street(FOLD, 3, true); /* facing turn cbet and folded! */
    tracker.end_hand();

    assert_eq!(tracker.turn_cbet_faced, 1);
    assert_eq!(tracker.turn_fold_to_cbet, 1);
    assert_eq!(tracker.turn_fold_to_cbet_ratio(), 1.0);

    /* River bet facing and folding */
    tracker.record_action_street(CALL_CHECK, 1, false);
    tracker.record_action_street(CALL_CHECK, 2, false);
    tracker.record_action_street(CALL_CHECK, 3, false);
    tracker.record_action_street(FOLD, 4, true); /* facing river bet and folded! */
    tracker.end_hand();

    /* Add realistic preflop folds so opponent represents an over-folding player */
    for _ in 0..4 {
        tracker.record_action_street(FOLD, 1, false);
        tracker.end_hand();
    }

    assert_eq!(tracker.river_bet_faced, 1);
    assert_eq!(tracker.river_fold_to_bet, 1);
    assert_eq!(tracker.river_fold_to_bet_ratio(), 1.0);

    /* Test turn barrel exploit in adjust_strategy_full */
    let base_strat = [0.30, 0.40, 0.10, 0.10, 0.10, 0.0];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT, RAISE_HALF_POT];
    /* Weak hand (bucket 10) on Turn (round 3) */
    let adj_turn = tracker.adjust_strategy(&ExploitRequest {
        blueprint_probs: &base_strat,
        legal_actions: &legal,
        bucket: 10,
        round: 3,
        ..Default::default()
    });
    let orig_raise: f64 = base_strat[3] + base_strat[4];
    let adj_raise: f64 = adj_turn[3] + adj_turn[4];
    assert!(
        adj_raise > orig_raise,
        "Facing turn over-folder, bot should fire 2nd barrel bluff more often (orig {orig_raise}, adj {adj_raise})"
    );
}

#[test]
fn test_opponent_sizing_tells() {
    let mut tracker = OpponentTracker::new();

    /* Record detailed actions with bet size and pot */
    /* Opponent bet 20 chips into 100 pot (20% pot -> small bet) */
    tracker.record_action_street_detailed(RAISE_MIN, 2, false, 20, 100);
    /* Opponent bet 50 chips into 100 pot (50% pot -> mid bet) */
    tracker.record_action_street_detailed(RAISE_HALF_POT, 2, false, 50, 100);
    /* Opponent bet 150 chips into 100 pot (150% pot -> large/overbet) */
    tracker.record_action_street_detailed(ALL_IN, 3, false, 150, 100);

    assert_eq!(tracker.sizing_small_bets, 1);
    assert_eq!(tracker.sizing_mid_bets, 1);
    assert_eq!(tracker.sizing_large_bets, 1);

    /* Record showdown with large bet and monster cards */
    let card = |rank, suit| Card { rank, suit };
    let opp_quads = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Clubs)];
    let board = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::King, Suit::Spades),
        card(Rank::Queen, Suit::Spades),
        card(Rank::Two, Suit::Diamonds),
    ];
    tracker.record_showdown_hand_detailed(opp_quads, &board, true, 800, 1000); /* 80% pot bet with Quads! */

    assert_eq!(tracker.showdown_large_monster_count, 1);
}

#[test]
fn test_opponent_positional_and_3bet_tracking() {
    let mut tracker = OpponentTracker::new();

    /* SB: 10 hands, 8 VPIP, 6 PFR */
    for _ in 0..6 {
        tracker.record_preflop_positional(true, true, true);
    }
    for _ in 0..2 {
        tracker.record_preflop_positional(true, true, false);
    }
    for _ in 0..2 {
        tracker.record_preflop_positional(true, false, false);
    }

    assert_eq!(tracker.sb_total_hands, 10);
    assert_eq!(tracker.sb_vpip_hands, 8);
    assert_eq!(tracker.sb_pfr_hands, 6);
    assert!((tracker.sb_vpip_ratio() - 0.80).abs() < 1e-6);
    assert!((tracker.sb_pfr_ratio() - 0.60).abs() < 1e-6);

    /* BB: 10 hands, 5 VPIP */
    for _ in 0..5 {
        tracker.record_preflop_positional(false, true, false);
    }
    for _ in 0..5 {
        tracker.record_preflop_positional(false, false, false);
    }
    assert_eq!(tracker.bb_total_hands, 10);
    assert_eq!(tracker.bb_vpip_hands, 5);
    assert!((tracker.bb_vpip_ratio() - 0.50).abs() < 1e-6);

    /* 3-bet facing: faced 4, folded 3 */
    tracker.record_facing_3bet(true);
    tracker.record_facing_3bet(true);
    tracker.record_facing_3bet(true);
    tracker.record_facing_3bet(false);
    assert_eq!(tracker.faced_3bet_count, 4);
    assert_eq!(tracker.fold_to_3bet_count, 3);
    assert!((tracker.fold_to_3bet_ratio() - 0.75).abs() < 1e-6);
}

#[test]
fn test_opponent_showdown_hand_tier_tracking() {
    let mut tracker = OpponentTracker::new();
    let card = |rank, suit| Card { rank, suit };

    let board = [
        card(Rank::King, Suit::Hearts),
        card(Rank::Eight, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
        card(Rank::Four, Suit::Spades),
        card(Rank::Nine, Suit::Diamonds),
    ];

    /* 1. Opponent shows down AKo (offsuit monster, index 91).
     * Must NOT be counted as loose trash!
     * Must be counted as a slowplay/trap. */
    let opp_ako = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    tracker.record_showdown_hand_detailed(opp_ako, &board, false, 0, 100);

    assert_eq!(
        tracker.showdown_loose_count, 0,
        "AKo is a Tier 1 hand and must NEVER increment showdown_loose_count"
    );
    assert_eq!(
        tracker.showdown_trap_count, 1,
        "AKo is a Tier 1 premium hand and should increment showdown_trap_count"
    );

    /* 2. Opponent shows down 72o (Tier 5 pure trash).
     * Must be counted as loose trash! */
    let opp_72o = [
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    ];
    tracker.record_showdown_hand_detailed(opp_72o, &board, false, 0, 100);

    assert_eq!(
        tracker.showdown_loose_count, 1,
        "72o is Tier 5 trash and must increment showdown_loose_count"
    );
}

#[test]
fn test_showdown_trap_false_positive_filtering_and_exploit() {
    use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;

    let mut tracker = OpponentTracker::new();
    let aa_hole = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Ace, Suit::Spades),
    ];
    let board = [
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Five, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Spades),
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Four, Suit::Clubs),
    ];

    /* 1. AA in a 3-bet pot (2 preflop raises) must NOT count as a trap */
    tracker.record_action_street_detailed(RAISE_HALF_POT, 1, false, 300, 150);
    tracker.record_action_street_detailed(RAISE_HALF_POT, 1, false, 900, 1200);
    tracker.record_showdown_hand_detailed(aa_hole, &board, false, 0, 1800);
    tracker.end_hand();
    assert_eq!(
        tracker.showdown_trap_count, 0,
        "AA in a 3-bet pot must not falsely increment showdown_trap_count"
    );

    /* 2. AA with a postflop raise must NOT count as a slowplay trap */
    tracker.record_action_street_detailed(RAISE_THIRD_POT, 2, false, 400, 400);
    tracker.record_showdown_hand_detailed(aa_hole, &board, false, 0, 1200);
    tracker.end_hand();
    assert_eq!(
        tracker.showdown_trap_count, 0,
        "AA with postflop aggression must not falsely increment showdown_trap_count"
    );

    /* 3. Passive AA (0-1 preflop raises, 0 postflop raises, opp_raised_river == false, last_bet_amt == 0) DOES count */
    tracker.record_action_street_detailed(CALL_CHECK, 1, false, 100, 150);
    tracker.record_action_street_detailed(CALL_CHECK, 2, false, 0, 200);
    tracker.record_showdown_hand_detailed(aa_hole, &board, false, 0, 200);
    tracker.end_hand();
    assert_eq!(tracker.showdown_trap_count, 1);

    /* One more genuine passive trap brings showdown_trap_count == 2 (ratio = 2/4 = 0.50 >= 0.25) */
    tracker.record_action_street_detailed(CALL_CHECK, 1, false, 100, 150);
    tracker.record_showdown_hand_detailed(
        [
            Card::new(Rank::King, Suit::Hearts),
            Card::new(Rank::King, Suit::Clubs),
        ],
        &board,
        false,
        0,
        200,
    );
    tracker.end_hand();
    assert_eq!(tracker.showdown_trap_count, 2);

    /* On River checked to us with marginal/medium equity (bucket 32), exploit trims bluff/big bets toward check-back */
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
    assert_eq!(tracker.total_hands, 20);
    assert_eq!(tracker.pfr_hands, 10);

    tracker.observe_blind_level(100);
    assert_eq!(tracker.total_hands, 20);

    tracker.observe_blind_level(200);
    assert!(
        tracker.total_hands < 20 && tracker.total_hands >= 12,
        "Expected soft decay on 2x blind jump (100 -> 200), got total_hands={}",
        tracker.total_hands
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
    assert_eq!(persisted.total_hands, 10);
    assert_eq!(persisted.vpip_hands, 10);
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
    let board = [
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Eight, Suit::Diamonds),
        Card::new(Rank::Four, Suit::Clubs),
        Card::new(Rank::Two, Suit::Spades),
        Card::new(Rank::Nine, Suit::Clubs),
    ];

    /* 1. River raise with AA (Overpair, cat == 1) must count as thin value pair, NEVER bluff */
    let aa_hole = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Ace, Suit::Clubs),
    ];
    tracker.record_showdown_hand_detailed(aa_hole, &board, true, 1400, 1000);
    assert_eq!(tracker.showdown_bluff_count, 0);
    assert_eq!(tracker.showdown_thin_value_pair_count, 1);
    assert_eq!(tracker.showdown_overbet_hands, 1);
    assert_eq!(tracker.showdown_overbet_value_hands, 1);

    /* 2. River raise with QJ air (cat == 0) increments showdown_bluff_count */
    let qj_hole = [
        Card::new(Rank::Queen, Suit::Spades),
        Card::new(Rank::Jack, Suit::Diamonds),
    ];
    tracker.record_showdown_hand_detailed(qj_hole, &board, true, 1500, 1000);
    assert_eq!(tracker.showdown_bluff_count, 1);
    assert_eq!(tracker.showdown_thin_value_pair_count, 1);
    assert_eq!(tracker.showdown_overbet_hands, 2);
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
    assert!((tracker.flop_cbet_freq() - 1.0).abs() < 1e-6);
    assert!((tracker.turn_barrel_freq() - 0.0).abs() < 1e-6);
    let prof = tracker
        .subgame_street_profile()
        .expect("street profile should populate after >= 3 facing samples");
    assert!(prof.turn_fold_to_barrel.unwrap_or(0.0) > 0.65);
    assert!(prof.river_fold_to_bet.unwrap_or(0.0) > 0.55);

    tracker.apply_session_decay(0.50);
    assert!(tracker.flop_cbet_made <= tracker.flop_cbet_opps);
    assert!(tracker.turn_barrel_made <= tracker.turn_barrel_opps);
    assert!(tracker.showdown_overbet_value_hands <= tracker.showdown_overbet_hands);
}

#[test]
fn test_check_tracking_tactical_profile_and_unified_post_selection() {
    /* 1. Verify postflop check counters, genuine call separation, and AFq */
    let mut tracker = OpponentTracker::new();
    for _ in 0..4 {
        tracker.record_action_street_detailed(CALL_CHECK, 1, true, 150, 300);
        tracker.record_action_street_detailed(CALL_CHECK, 2, false, 0, 400);
        tracker.record_action_street_detailed(CALL_CHECK, 3, false, 0, 400);
        tracker.record_action_street_detailed(CALL_CHECK, 4, false, 0, 400);
        tracker.end_hand();
    }
    assert_eq!(tracker.check_count, 12);
    assert_eq!(tracker.flop_checks, 4);
    assert_eq!(tracker.turn_checks, 4);
    assert_eq!(tracker.river_checks, 4);
    assert_eq!(tracker.genuine_call_count(), 4);
    assert!((tracker.check_ratio() - 0.75).abs() < 1e-6);
    assert!(tracker.afq_ratio() < 1e-6);
    assert!(tracker.postflop_afq() < 1e-6);

    tracker.apply_session_decay(0.50);
    assert!(tracker.check_count <= tracker.call_count);

    /* 2. Verify TacticalProfile unifies behavioral classification */
    let mut station = make_passive_station();
    for _ in 0..8 {
        station.record_preflop_positional(false, true, false);
    }
    let profile = station.tactical_profile();
    assert!(profile.is_passive_station);
    assert!(profile.is_sticky_caller);
    assert!(profile.is_ultra_sticky_bb);

    /* 3. Verify unified post-selection guards run directly inside select_action_traced */
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
        let parsed: GuardId = id.as_str().parse().expect("valid canonical GuardId");
        assert_eq!(parsed, id);
    }

    let mut mask = GuardSet::EMPTY;
    assert!(mask.is_empty());
    mask.insert(GuardId::ShoveDefenseFold);
    mask.insert(GuardId::PostBarrelFold);
    assert_eq!(mask.len(), 2);
    assert!(mask.contains(GuardId::ShoveDefenseFold));
    assert!(!mask.contains(GuardId::PfButtonSteal));
    let collected: Vec<GuardId> = mask.into_iter().collect();
    assert_eq!(
        collected,
        vec![GuardId::ShoveDefenseFold, GuardId::PostBarrelFold]
    );

    let parsed_spec = GuardSet::from_disable_spec("shove_defense_fold, POST_BARREL_FOLD")
        .expect("comma-separated guard spec parses");
    assert_eq!(parsed_spec, mask);
    assert!(GuardSet::from_disable_spec("UNKNOWN_GUARD").is_err());

    let mut tracker = OpponentTracker::new();
    tracker.set_disabled_guards(["SHOVE_DEFENSE_FOLD"]);
    assert!(!tracker.is_guard_enabled(GuardId::ShoveDefenseFold));
    assert!(tracker.is_guard_enabled(GuardId::PostBarrelFold));
    tracker.set_disabled_guards(["ALL"]);
    assert!(!tracker.is_guard_enabled(GuardId::PfDisciplineGuard));

    assert!(PostflopBucket(33).has_pair_redraw());
    assert!(PostflopBucket(38).has_pair_redraw());
    assert!(!PostflopBucket(33).is_bare_weak_or_mid_pair());
    assert!(PostflopBucket(32).is_bare_weak_or_mid_pair());
    assert!(PostflopBucket(36).is_clean_mid_pair());
    assert!(PostflopBucket(73).is_board_chop_or_idiot_straight());
    assert!(PostflopBucket(25).is_multiway_nut_draw());
    assert!(!PostflopBucket(24).is_multiway_nut_draw());

    assert_eq!(
        "fish".parse::<OpponentArchetype>().ok(),
        Some(OpponentArchetype::CallingStation)
    );
    assert_eq!(
        "nit".parse::<OpponentArchetype>().ok(),
        Some(OpponentArchetype::Rock)
    );
    assert_eq!(
        "model".parse::<OpponentArchetype>().ok(),
        Some(OpponentArchetype::SelfPlay)
    );
    assert!("callng_station".parse::<OpponentArchetype>().is_err());

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
