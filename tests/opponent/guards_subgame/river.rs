use crate::opponent::{
    LEGAL_FULL, LEGAL_OPEN, PROBS_FOLD_HEAVY, PROBS_OPEN_JAM, PROBS_RERAISE_HEAVY, Spot, board,
    hole, make_overbet_bluffer, make_ultra_nit,
};
use lil_poker_mccfr::cfr::opponent_model::{GuardId, OpponentTracker};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[test]
fn test_subgame_river_paired_and_four_flush_folds() {
    const JAM_HEAVY: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.25, 0.0, 0.10, 0.10, 0.50];
    const CALL_HEAVY_90: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.90, 0.05, 0.0, 0.0, 0.0];
    const CALL_HEAVY_95: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];

    let neutral = OpponentTracker::new();
    let ultra_nit = make_ultra_nit();
    let mut rng = SmallRng::seed_from_u64(20_261_119);

    /* Js Ts on 5s Jc 8d 9s 5d paired connected River: neutral never jams/raises, ultra-nit folds */
    let jt_spot = Spot::post("Js Ts", "5s Jc 8d 9s 5d")
        .pot(5625, 13125)
        .stacks(300, 6359)
        .subgame();
    let (act_neutral, _) = jt_spot.once(&neutral, &JAM_HEAVY, &LEGAL_FULL, &mut rng);
    assert!(
        act_neutral < RAISE_MIN,
        "Js Ts on paired dangerous river must never jam/raise, got {act_neutral}"
    );
    let (act_nit, _) = jt_spot.once(&ultra_nit, &JAM_HEAVY, &LEGAL_FULL, &mut rng);
    assert_eq!(
        act_nit, FOLD,
        "Js Ts vs ultra-nit triple barrel on paired dangerous river must FOLD"
    );

    let mut check_fold = |spot: Spot, tracker: &OpponentTracker, probs: &[f64], msg: &str| {
        let (act, tr) = spot.subgame().once(tracker, probs, &LEGAL_FULL, &mut rng);
        let reasons = tr.guard_reasons;
        assert_eq!(act, FOLD, "{msg} (reasons: {reasons:?})");
    };
    check_fold(
        Spot::post("Qc Jc", "Qd Kd 6d 4d 5s")
            .pot(375, 1125)
            .stacks(150, 6256),
        &neutral,
        &CALL_HEAVY_90,
        "Qc Jc without diamond on 4-diamond river must fold",
    );
    check_fold(
        Spot::post("Qh Js", "3c Kh Qc 3s Ac")
            .pot(3750, 11250)
            .stacks(600, 8586),
        &ultra_nit,
        &CALL_HEAVY_90,
        "3rd pair Qh Js vs ultra-nit 3rd barrel must fold",
    );
    check_fold(
        Spot::post("Qh Qd", "Ks 9h 6c 5h 9s")
            .pot(4648, 9938)
            .stacks(200, 4648),
        &neutral,
        &CALL_HEAVY_95,
        "Qh Qd underpair on Ks 9h 6c 5h 9s must fold to river jam in subgame",
    );
    check_fold(
        Spot::post("Js Jh", "6h 3c 6s Ah 5d")
            .pot(4600, 15100)
            .stacks(150, 4900)
            .opp(Some(0)),
        &neutral,
        &CALL_HEAVY_95,
        "Underpair on paired A-high river must fold to explicit opp all-in",
    );
    let baby_flush = Spot::post("5h 4h", "Ah Jh 6h Td 8h")
        .pot(1498, 3498)
        .stacks(300, 9000);
    assert_eq!(
        baby_flush.bucket, 79,
        "5-high flush on 4-flush board maps to bucket 79"
    );
    check_fold(
        baby_flush,
        &neutral,
        &CALL_HEAVY_95,
        "Baby flush (bucket 79) on 4-flush river must fold to 75% pot bet",
    );
}

#[test]
fn test_subgame_river_bluff_catch_and_non_fold_invariants() {
    const CALL_HEAVY_95: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];
    const PURE_CALL: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0];

    let neutral = OpponentTracker::new();
    let overbet = make_overbet_bluffer();
    assert!(overbet.is_overbet_bluffer());
    let mut rng = SmallRng::seed_from_u64(20_261_120);

    /* Dry short-SPR river 2nd-pair facing ~82% pot all-in (pot_odds ~31%, <10 BB) must NOT auto-fold */
    let short_spr_spot = Spot::post("8d 6h", "Jh 5s 8s 2d Qc")
        .pot(1961, 4361)
        .stacks(300, 1961)
        .opp(Some(0))
        .subgame();
    assert_eq!(short_spr_spot.bucket, 35);
    let (act_86, tr_86) = short_spr_spot.once(&neutral, &CALL_HEAVY_95, &LEGAL_FULL, &mut rng);
    let reasons_86 = tr_86.guard_reasons;
    assert!(
        act_86 != FOLD && !reasons_86.contains(GuardId::ShoveDefenseFold),
        "Dry short-SPR river 2nd pair facing 82% pot bet must not auto-fold (reasons: {reasons_86:?})"
    );

    /* Q-high flush (bucket >= 80) on 4-flush river must not trigger ShoveDefenseFold or fold */
    let qh_flush = Spot::post("Qh 2c", "Ah Jh 6h Td 8h")
        .pot(1498, 3498)
        .stacks(300, 9000)
        .subgame();
    assert!(
        qh_flush.bucket >= 80,
        "Q-high flush must stay in bucket >= 80"
    );
    let (act_qh, tr_qh) = qh_flush.once(&neutral, &PURE_CALL, &LEGAL_FULL, &mut rng);
    assert!(
        !tr_qh.guard_reasons.contains(GuardId::ShoveDefenseFold) && act_qh != FOLD,
        "Q-high flush on 4-flush river must not fold or trigger ShoveDefenseFold"
    );

    /* Ks 7d bluff-catcher vs overbet bluffer on paired river must call frequently */
    let (counts, _) = Spot::post("Ks 7d", "3d 9c 7h Js Jh")
        .pot(4500, 7500)
        .stacks(150, 5400)
        .subgame()
        .sample(&overbet, &PROBS_FOLD_HEAVY, &LEGAL_FULL, 100, &mut rng);
    let calls = counts[CALL_CHECK as usize];
    assert!(
        calls >= 45,
        "Ks 7d bluff-catcher vs overbet spammer must call frequently, got {calls}/100"
    );
}

#[test]
fn test_subgame_river_showdown_check_backs_and_raise_caps() {
    const OVERBET_RAW: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.10, 0.0, 0.0, 0.90, 0.0];
    const MIN_BLUFF_RAW: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.15, 0.85, 0.0, 0.0, 0.0];
    const RAISE_HEAVY_FLUSH: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.30, 0.0, 0.0, 0.15, 0.50];
    const RAISE_HEAVY_2P: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.20, 0.25, 0.30, 0.0, 0.20];

    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_121);

    /* King-high showdown value and pure air on paired river must check back */
    let check_cases = [
        (
            Spot::post("Ks 5h", "Td 8c 8h 2d 4h")
                .pot(0, 500)
                .stacks(100, 12253),
            &OVERBET_RAW,
            "King-high showdown value must check back instead of 125% overbet bluff",
        ),
        (
            Spot::post("Qc 9c", "Jd 4h Ac Tc Ts")
                .pot(0, 1250)
                .stacks(100, 9663),
            &MIN_BLUFF_RAW,
            "Pure air must check back instead of 25% micro-bluff into paired river",
        ),
    ];
    for (spot, probs, msg) in check_cases {
        let (act, _) = spot.subgame().once(&neutral, probs, &LEGAL_OPEN, &mut rng);
        assert_eq!(act, CALL_CHECK, "{msg}");
    }

    let mut check_cap =
        |spot: Spot, legal: &[u8], probs: &[f64], trials: usize, mode: &str, msg: &str| {
            let spot = spot.subgame();
            for _ in 0..trials {
                let (act, tr) = spot.once(&neutral, probs, legal, &mut rng);
                let reasons = tr.guard_reasons;
                match mode {
                    "no_overbet_or_jam" => {
                        assert!(act != RAISE_HALF_POT && act != ALL_IN, "{msg} (got {act})")
                    }
                    "no_all_in" => assert_ne!(act, ALL_IN, "{msg}"),
                    _ => assert!(act < RAISE_MIN, "{msg} (got {act}, reasons: {reasons:?})"),
                }
            }
        };
    check_cap(
        Spot::post("Td 9h", "Jh 9c 2d 6h Ks")
            .pot(0, 1320)
            .stacks(150, 14000),
        &LEGAL_OPEN,
        &OVERBET_RAW,
        40,
        "no_overbet_or_jam",
        "Td 9h middle pair must never 125% overbet river",
    );
    check_cap(
        Spot::post("Qh 4h", "2h 4d Th 5h 3h")
            .pot(4572, 9500)
            .stacks(200, 6882),
        &LEGAL_FULL,
        &RAISE_HEAVY_FLUSH,
        80,
        "no_raise",
        "Non-nut Q-high flush on 4-flush river facing raise must never reraise/jam",
    );
    check_cap(
        Spot::post("Qc Ts", "6s 4s Tc 9c Qs")
            .pot(2478, 5782)
            .stacks(150, 8300),
        &LEGAL_FULL,
        &RAISE_HEAVY_2P,
        40,
        "no_raise",
        "Two pair without flush facing 75% bet on 3-flush river must never raise",
    );
    let paired_flush = Spot::post("Td 4d", "6h Qh Qd 5d 3d")
        .pot(749, 1997)
        .stacks(150, 6172);
    assert_eq!(paired_flush.bucket, 81);
    check_cap(
        paired_flush,
        &LEGAL_FULL,
        &PROBS_RERAISE_HEAVY,
        40,
        "no_raise",
        "Non-nut flush on paired 3-flush river facing lead must call/fold, not raise",
    );
    check_cap(
        Spot::post("Qc As", "Ac 8d Tc 4c 5c")
            .pot(0, 3328)
            .stacks(100, 7750),
        &LEGAL_OPEN,
        &PROBS_OPEN_JAM,
        40,
        "no_all_in",
        "2nd-nut flush on 4-flush river must not open-jam >2x pot ALL_IN",
    );
}

#[test]
fn test_subgame_river_value_bet_floors() {
    const PASSIVE_CHECK: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.85, 0.0, 0.15, 0.0, 0.0];
    const THIN_CHECK_70: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.70, 0.25, 0.05, 0.0, 0.0];
    const THIN_CHECK_85: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.85, 0.10, 0.05, 0.0, 0.0];

    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_119);

    let mut check_floor = |spot: Spot, probs: &[f64], min_bets: usize, mode: &str, msg: &str| {
        let spot = spot.subgame();
        let (counts, _) = spot.sample(&neutral, probs, &LEGAL_OPEN, 80, &mut rng);
        let bets = match mode {
            "min_only" => counts[RAISE_MIN as usize],
            "compact_only" => counts[RAISE_MIN as usize] + counts[RAISE_THIRD_POT as usize],
            _ => counts[RAISE_MIN as usize..].iter().sum(),
        };
        let bucket = spot.bucket;
        assert!(bets >= min_bets, "{msg} (bucket {bucket}, got {bets}/80)");
    };
    check_floor(
        Spot::post("Kd 8c", "2h 5s 4h 7c 6d")
            .pot(0, 400)
            .stacks(200, 5189),
        &PASSIVE_CHECK,
        45,
        "any_bet",
        "Nut Straight must value-bet majority of the time",
    );
    check_floor(
        Spot::post("Tc 8c", "Kc 8h 8d Ad 4h")
            .pot(0, 1800)
            .stacks(100, 8000),
        &PASSIVE_CHECK,
        45,
        "any_bet",
        "River Trips on safe paired board must value-bet frequently",
    );
    check_floor(
        Spot::post("Qh Tc", "Kd Ts 6c 3h 2s")
            .pot(0, 800)
            .stacks(100, 8000),
        &THIN_CHECK_70,
        30,
        "min_only",
        "Clean unpaired River middle pair must thin-value bet frequently",
    );
    check_floor(
        Spot::post("Th 8s", "Tc 8d 4s 2c 3h")
            .pot(0, 1500)
            .stacks(150, 8000),
        &THIN_CHECK_85,
        35,
        "compact_only",
        "River genuine two-pair must fire thin value frequently instead of check-back",
    );
}

#[test]
fn test_subgame_overbet_bluffer_detection_and_river_call() {
    const CALL_HEAVY_95: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];

    let mut rng = SmallRng::seed_from_u64(20_261_123);

    /* 1. 2 overbets in first 2 postflop bets marks is_overbet_bluffer() and protects Qc Js */
    let mut early_overbet = OpponentTracker::new();
    early_overbet.record_action_street_detailed(RAISE_HALF_POT, 3, false, 2400, 1600);
    early_overbet.record_action_street_detailed(RAISE_HALF_POT, 4, false, 9184, 2666);
    assert!(early_overbet.is_overbet_bluffer());

    let (act_qj, tr_qj) = Spot::post("Qc Js", "8h 5d Td 8s Qs")
        .pot(9184, 11850)
        .stacks(100, 9184)
        .opp(Some(0))
        .subgame()
        .once(&early_overbet, &CALL_HEAVY_95, &LEGAL_FULL, &mut rng);
    let reasons_qj = tr_qj.guard_reasons;
    assert!(
        act_qj != FOLD && !reasons_qj.contains(GuardId::ShoveDefenseFold),
        "Top pair Qc Js vs early overbet spammer must not force-fold (reasons: {reasons_qj:?})"
    );

    /* 2. Low-aggression polarized bettor with sporadic 150% river bets is NOT flagged */
    let mut balanced = OpponentTracker::new();
    for _ in 0..8 {
        balanced.record_action_street_detailed(CALL_CHECK, 1, true, 0, 300);
        balanced.record_action_street_detailed(CALL_CHECK, 2, false, 0, 500);
        balanced.record_action_street_detailed(CALL_CHECK, 3, false, 0, 500);
        balanced.end_hand();
    }
    for _ in 0..4 {
        balanced.record_action_street_detailed(RAISE_THIRD_POT, 2, false, 300, 500);
        balanced.record_action_street_detailed(RAISE_HALF_POT, 4, false, 1650, 1100);
        balanced.end_hand();
    }
    assert!(
        !balanced.is_overbet_bluffer(),
        "Low-aggression opponent (raise_ratio < 0.30) with sporadic overbets must not be flagged"
    );
    assert!(
        balanced.subgame_bluff_multiplier().is_none_or(|m| m < 1.0),
        "Balanced polarized bettor must not receive inflated subgame bluff multiplier"
    );

    /* 3. Value-heavy trapper showing monsters at showdown vetoes is_overbet_bluffer() */
    let mut trapper = OpponentTracker::new();
    let hole_set = hole("7s 7h");
    let board_set = board("Ac 7d 2s Kh 4c");
    for _ in 0..4 {
        trapper.record_action_street_detailed(RAISE_HALF_POT, 4, false, 1500, 1000);
        trapper.record_showdown_hand_detailed(hole_set, &board_set, true, 1500, 1000);
        trapper.end_hand();
    }
    assert!(
        !trapper.is_overbet_bluffer(),
        "Showdown-confirmed value monster overbettor must be vetoed from is_overbet_bluffer()"
    );

    /* 4. Hyper-aggressive opponent (raise_ratio >= 0.45) mixing 37.5% overbets flags */
    let mut mixed_hyper = OpponentTracker::new();
    for _ in 0..3 {
        mixed_hyper.record_action_street_detailed(RAISE_HALF_POT, 2, false, 1500, 1000);
        mixed_hyper.record_action_street_detailed(RAISE_MIN, 2, false, 300, 1000);
        mixed_hyper.end_hand();
    }
    for _ in 0..2 {
        mixed_hyper.record_action_street_detailed(RAISE_MIN, 2, false, 300, 1000);
        mixed_hyper.record_action_street_detailed(CALL_CHECK, 2, false, 0, 1000);
        mixed_hyper.end_hand();
    }
    mixed_hyper.record_action_street_detailed(CALL_CHECK, 2, false, 0, 1000);
    mixed_hyper.record_action_street_detailed(FOLD, 2, true, 0, 1000);
    assert!(
        mixed_hyper.raise_ratio() >= 0.45 && mixed_hyper.is_overbet_bluffer(),
        "Hyper-aggressive opponent with 37.5% postflop overbets must trigger is_overbet_bluffer()"
    );
}
