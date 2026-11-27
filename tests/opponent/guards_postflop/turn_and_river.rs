use super::super::{LEGAL_FULL, LEGAL_OPEN, LEGAL_SHOVE, Spot, board, hole, make_passive_station};
use lil_poker_mccfr::cfr::abstraction::detect_river_nut_blocker;
use lil_poker_mccfr::cfr::opponent_model::{GuardId, OpponentTracker};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use rand::SeedableRng;
use rand::rngs::SmallRng;

fn board_spot(round: u8, bucket: usize, b: &str) -> Spot {
    let mut s = Spot::bucket_only(round, bucket);
    s.board_cards = board(b);
    s
}

fn with_bucket(mut s: Spot, bucket: usize) -> Spot {
    s.bucket = bucket;
    s
}

#[test]
fn test_river_blocker_detection_and_bluffing() {
    let river_board = board("Ks 9s 4s 2h 7d");
    assert!(
        detect_river_nut_blocker(&hole("As 3c"), &river_board),
        "Ace of spades on 3-spade board must be detected as nut blocker"
    );
    assert!(
        !detect_river_nut_blocker(&hole("Qc 3c"), &river_board),
        "Hand without spade should not trigger nut blocker"
    );

    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(123);
    let raw_probs = [0.40, 0.40, 0.0, 0.10, 0.10, 0.0];
    let legal = [FOLD, CALL_CHECK, RAISE_THIRD_POT, RAISE_HALF_POT];
    let s1 = with_bucket(Spot::post("As 3c", "Ks 9s 4s 2h 7d"), 10).pot(0, 1000);
    let s2 = with_bucket(Spot::post("Qc 3c", "Ks 9s 4s 2h 7d"), 10).pot(0, 1000);
    let (mut raises_with_blocker, mut raises_without_blocker) = (0, 0);
    for _ in 0..200 {
        let (a1, _) = s1.once(&tracker, &raw_probs, &legal, &mut rng);
        if a1 == RAISE_THIRD_POT || a1 == RAISE_HALF_POT {
            raises_with_blocker += 1;
        }
        let (a2, _) = s2.once(&tracker, &raw_probs, &legal, &mut rng);
        if a2 == RAISE_THIRD_POT || a2 == RAISE_HALF_POT {
            raises_without_blocker += 1;
        }
    }
    assert!(
        raises_with_blocker > raises_without_blocker,
        "River bluffing frequency with nut blocker ({raises_with_blocker}) must exceed without blocker ({raises_without_blocker})"
    );
}

#[test]
fn test_shove_defense_wet_turn_and_river_four_straight() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);
    let s1 = with_bucket(Spot::post("Jh Jc", "4d 6d 7c 3c"), 43).pot(3661, 4500);
    let (chosen_jj_turn, _) = s1.once(
        &tracker,
        &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
        &[FOLD, CALL_CHECK],
        &mut rng,
    );
    assert_eq!(
        chosen_jj_turn, FOLD,
        "Overpair JJ on a 4-to-straight Turn board facing all-in raise must fold!"
    );

    let s2 = board_spot(4, 68, "Kh Ad 2h Qs Jd")
        .pot(6725, 9975)
        .stacks(200, 0);
    let (act_set, _) = s2.once(
        &tracker,
        &[0.05, 0.90, 0.05, 0.0, 0.0, 0.0],
        &LEGAL_SHOVE,
        &mut rng,
    );
    assert_eq!(
        act_set, FOLD,
        "Set on a 4-to-a-straight river board must fold to a river shove!"
    );
}

#[test]
fn test_shove_defense_river_overcards_and_flop_air_cap() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);

    let s1 = with_bucket(Spot::post("Jh Jc", "Qc Ks 3h Ad 6d"), 35).pot(3254, 4000);
    let (chosen_jj_river, _) = s1.once(
        &tracker,
        &[0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
        &[FOLD, CALL_CHECK],
        &mut rng,
    );
    assert_eq!(
        chosen_jj_river, FOLD,
        "Fourth pair on Q-K-3-A-6 River board facing heavy all-in call must fold!"
    );

    let station = make_passive_station();
    let s2 = Spot::bucket_only(2, 6).pot(250, 500);
    let (act, _) = s2.once(
        &station,
        &[0.05, 0.05, 0.20, 0.20, 0.20, 0.30],
        &LEGAL_FULL,
        &mut rng,
    );
    assert!(
        act < RAISE_MIN,
        "Pure air facing a flop bet must not bluff-raise or shove, got {act}"
    );

    /* Flop small 1 BB bet (~25% pot odds) with Ace-high overcards must not force 100% fold */
    let s3 = Spot::post("Ac 9d", "Qs 7d 2c")
        .pot(100, 300)
        .stacks(100, 9000);
    let (act_small_stab, _) = s3.once(
        &tracker,
        &[0.05, 0.90, 0.05, 0.0, 0.0, 0.0],
        &LEGAL_FULL,
        &mut rng,
    );
    assert_ne!(
        act_small_stab, FOLD,
        "Ace-high two overcards facing 1 BB flop bet must not force-fold"
    );

    /* Loose sticky caller (calls and raises ~30%, low folds < 20%) suppresses pure-air flop c-bets */
    let mut sticky = OpponentTracker::new();
    for _ in 0..14 {
        sticky.record_action_street(CALL_CHECK, 2, true);
    }
    for _ in 0..6 {
        sticky.record_action_street(RAISE_MIN, 1, false);
    }
    sticky.record_action_street(FOLD, 2, true);
    let s4 = Spot::bucket_only(2, 4).pot(0, 300).stacks(100, 9000);
    let (counts, _) = s4.sample(
        &sticky,
        &[0.0, 0.15, 0.35, 0.30, 0.20, 0.0],
        &LEGAL_OPEN,
        80,
        &mut rng,
    );
    let sticky_checks = counts[CALL_CHECK as usize];
    assert!(
        sticky_checks >= 60,
        "Pure air vs sticky caller on flop must check mostly (got {sticky_checks}/80 checks)"
    );
}

#[test]
fn test_postflop_barrel_folds_and_turn_pot_commit() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(2026);
    let call_heavy = [0.10, 0.90, 0.0, 0.0, 0.0, 0.0];

    let s1 = board_spot(2, 30, "9h 7c 3h").pot(800, 2000).stacks(200, 0);
    let (act_bottom, _) = s1.once(&tracker, &call_heavy, &LEGAL_FULL, &mut rng);
    assert_eq!(
        act_bottom, FOLD,
        "Bare bottom pair must fold to a 2/3 pot flop bet!"
    );

    let s2 = board_spot(3, 42, "4s Ks 8s 6d")
        .pot(2900, 7250)
        .stacks(200, 0);
    let (act_tp_mono, _) = s2.once(&tracker, &call_heavy, &LEGAL_FULL, &mut rng);
    assert_eq!(
        act_tp_mono, FOLD,
        "Bare top pair mid kicker without flush redraw on 3-flush turn must fold to heavy barrel!"
    );

    let s3 = board_spot(3, 58, "7c 3d 7d 5d")
        .pot(600, 9400)
        .stacks(200, 0);
    let (act_committed, _) = s3.once(
        &tracker,
        &[0.95, 0.05, 0.0, 0.0, 0.0, 0.0],
        &LEGAL_SHOVE,
        &mut rng,
    );
    assert_ne!(
        act_committed, FOLD,
        "Calling 600 into 9400 (6% pot odds) with overpair must never fold!"
    );

    /* Flop 3-bet pot underpair below 2 overcards facing large barrel must fold */
    let s4 = Spot::post("5c 5s", "Ac Tc 2h")
        .pot(2206, 5206)
        .stacks(200, 6682);
    let (act_3bp, _) = s4.once(
        &tracker,
        &[0.05, 0.95, 0.0, 0.0, 0.0, 0.0],
        &LEGAL_FULL,
        &mut rng,
    );
    assert_eq!(
        act_3bp, FOLD,
        "Underpair 55 under A-T in bloated 3-bet pot must fold to 11 BB flop barrel"
    );
}

#[test]
fn test_priority_tier_ordering_and_decision_trace_reasons() {
    let station = make_passive_station();
    let mut rng = SmallRng::seed_from_u64(999);
    let spot = board_spot(4, 76, "5h 9s 7d 9h 8h")
        .pot(0, 2000)
        .stacks(200, 0);
    let (act, trace) = spot.once(
        &station,
        &[0.0, 0.85, 0.05, 0.05, 0.05, 0.0],
        &LEGAL_OPEN,
        &mut rng,
    );
    let reasons = trace.guard_reasons;
    assert!(
        act != ALL_IN && act != RAISE_HALF_POT,
        "Priority Tier 2 cap must override Tier 1 passive-station boost, got {act}"
    );
    assert!(
        reasons.contains(GuardId::PassiveStationBoost)
            && reasons.contains(GuardId::PostPairedFlushStraightCap),
        "DecisionTrace must record both Tier 1 and Tier 2 reasons: {reasons:?}"
    );
}

#[test]
fn test_river_set_overbet_cap_on_dangerous_board() {
    let tracker = OpponentTracker::new();
    let spot = Spot::post("9h 9c", "Jc Td 9s 4h 2s")
        .pot(1200, 1500)
        .stacks(200, 10000);
    let bucket = spot.bucket;
    assert!(
        (68..=72).contains(&bucket),
        "Set on 3-straight river board should fall in set bucket 68..=72, got {bucket}"
    );

    let policy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.15, 0.15, 0.0, 0.30, 0.40];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN, RAISE_HALF_POT, ALL_IN];
    let mut rng = SmallRng::seed_from_u64(4242);
    let (counts, reasons) = spot.sample(&tracker, &policy, &legal, 100, &mut rng);
    assert_eq!(
        counts[CALL_CHECK as usize], 100,
        "Set facing a 6 BB river bet on dangerous 3-straight board must flat-call (reasons: {reasons:?})"
    );
}

#[test]
fn test_queen_high_flush_no_fold_and_monotone_j_fd() {
    let mut tracker = OpponentTracker::new();
    for _ in 0..10 {
        tracker.record_action_street_detailed(RAISE_HALF_POT, 1, false, 100, 300);
        tracker.end_hand();
    }
    let mut rng = SmallRng::seed_from_u64(42);

    let s_flush = Spot::post("Qs Ts", "7s Td 5s 3s")
        .pot(8140, 3200)
        .stacks(100, 8660);
    assert_eq!(s_flush.bucket, 81);
    let (act_flush, trace_flush) = s_flush.once(
        &tracker,
        &[0.85, 0.15, 0.0, 0.0, 0.0, 0.0],
        &[FOLD, CALL_CHECK],
        &mut rng,
    );
    let reasons = trace_flush.guard_reasons;
    assert_eq!(
        act_flush, CALL_CHECK,
        "Queen-high flush on unpaired 3-flush turn must call shove (reasons: {reasons:?})"
    );

    let s_fd = Spot::post("Kd Jh", "Th 7h 4h")
        .pot(400, 800)
        .stacks(150, 8375);
    let b_j_fd = s_fd.bucket;
    assert!(
        b_j_fd >= 19,
        "Kd Jh must retain Flush Draw bucket >= 19, got {b_j_fd}"
    );
    let (act_fd, _) = s_fd.once(
        &tracker,
        &[0.05, 0.85, 0.10, 0.0, 0.0, 0.0],
        &LEGAL_FULL,
        &mut rng,
    );
    assert_ne!(
        act_fd, FOLD,
        "Jack-high FD + overs on monotone flop must not fold"
    );
}

#[test]
fn test_pair_plus_nfd_turn_no_fold_and_low_spr_river_nut_blocker() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_116);

    let s_turn = Spot::post("As 8c", "5c 8s Ks 6s")
        .pot(2400, 4000)
        .stacks(200, 4318);
    assert_eq!(s_turn.bucket, 26);
    let (counts_turn, reasons_turn) = s_turn.sample(
        &tracker,
        &[0.25, 0.70, 0.05, 0.0, 0.0, 0.0],
        &[FOLD, CALL_CHECK, RAISE_MIN, ALL_IN],
        60,
        &mut rng,
    );
    for forbidden in [
        GuardId::ShoveDefenseFold,
        GuardId::TurnDrawOddsFold,
        GuardId::PostBarrelFold,
        GuardId::RangeEquityShoveFold,
    ] {
        assert!(
            !reasons_turn.contains(forbidden),
            "Pair + NFD (bucket 26) triggered fold guard on 3-flush Turn (reasons: {reasons_turn:?})"
        );
    }
    let turn_calls = counts_turn[CALL_CHECK as usize];
    assert!(
        turn_calls >= 35,
        "Pair + NFD (bucket 26) must call predominantly on 3-flush Turn (got {turn_calls}/60)"
    );

    let s_river = Spot::post("Ah Kc", "3h 2h 7h Td Jc")
        .pot(0, 6878)
        .stacks(150, 3619);
    let (counts_river, reasons_river) = s_river.sample(
        &tracker,
        &[0.0, 0.85, 0.05, 0.10, 0.0, 0.0],
        &LEGAL_OPEN,
        80,
        &mut rng,
    );
    let blocker_bluffs: usize = counts_river[RAISE_MIN as usize..].iter().sum();
    assert!(
        reasons_river.contains(GuardId::RiverNutBlockerBluff) && blocker_bluffs >= 10,
        "Low-SPR river with Ace nut blocker must bluff sometimes (got {blocker_bluffs}/80)"
    );
}

#[test]
fn test_monster_value_raise_and_river_halfpot_tp() {
    let mut rng = SmallRng::seed_from_u64(4242);
    let tracker = OpponentTracker::new();
    let s1 = Spot::bucket_only(4, 85).pot(500, 2000);
    let (counts, _) = s1.sample(
        &tracker,
        &[0.0, 0.05, 0.0, 0.0, 0.95, 0.0],
        &LEGAL_FULL,
        50,
        &mut rng,
    );
    let raises: usize = counts[RAISE_MIN as usize..].iter().sum();
    assert!(
        raises >= 40,
        "Monster bucket 85 must keep value-raising (got {raises}/50)"
    );

    let s2 = board_spot(4, 47, "As Td 6c 3h 2s").pot(500, 1500);
    let (act_tp, _) = s2.once(
        &tracker,
        &[0.05, 0.95, 0.0, 0.0, 0.0, 0.0],
        &LEGAL_FULL,
        &mut rng,
    );
    assert_eq!(
        act_tp, CALL_CHECK,
        "Top Pair must call standard 5 BB half-pot river bet"
    );
}
