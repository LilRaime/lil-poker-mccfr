use crate::opponent::{
    LEGAL_FULL, LEGAL_OPEN, LEGAL_SHOVE, PROBS_FOLD_HEAVY, PROBS_OPEN_JAM, Spot, make_aggro_maniac,
    make_overbet_bluffer,
};
use lil_poker_mccfr::cfr::opponent_model::{GuardId, OpponentTracker};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[test]
fn test_subgame_turn_shove_and_overbet_folds() {
    type FoldCase<'a> = (Spot, &'a OpponentTracker, &'a [u8], &'a [f64], &'a str);
    const CALL_HEAVY: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.95, 0.0, 0.0, 0.0, 0.0];
    const CALL_HEAVY_90: [f64; HOLDEM_NUM_ACTIONS] = [0.10, 0.90, 0.0, 0.0, 0.0, 0.0];

    let neutral = OpponentTracker::new();
    let overbet = make_overbet_bluffer();
    let maniac = make_aggro_maniac();
    let mut rng = SmallRng::seed_from_u64(20_261_120);

    let cases: Vec<FoldCase<'_>> = vec![
        (
            Spot::post("As 4c", "5s 6c 2d Ah")
                .pot(8314, 9010)
                .stacks(100, 8314),
            &neutral,
            &LEGAL_FULL,
            &CALL_HEAVY,
            "As 4c must fold to 11.9x pot turn jam in subgame",
        ),
        (
            Spot::post("7d Ks", "6h Jc 7s Jh")
                .pot(1906, 4578)
                .stacks(300, 3285),
            &neutral,
            &LEGAL_FULL,
            &CALL_HEAVY,
            "Turn middle pair on paired J-high board facing committing barrel must fold",
        ),
        (
            Spot::post("4s 4c", "Jh 7c 7h 5h")
                .pot(1500, 4500)
                .stacks(400, 7164),
            &neutral,
            &LEGAL_FULL,
            &CALL_HEAVY,
            "Underpair without flush suit on paired 3-flush turn must fold to 3.75 BB bet",
        ),
        (
            Spot::post("Js Jh", "6h 3c 6s Ah")
                .pot(3375, 7125)
                .stacks(150, 7975)
                .opp(Some(4600)),
            &neutral,
            &LEGAL_FULL,
            &CALL_HEAVY,
            "Underpair on paired A-high turn must fold to 90% pot barrel",
        ),
        (
            Spot::post("Ks 7d", "Kh Jc 9s Td")
                .pot(2812, 6262)
                .stacks(300, 5649),
            &neutral,
            &LEGAL_FULL,
            &CALL_HEAVY,
            "Bare top pair on 4-straight turn must fold to 80% pot barrel",
        ),
        (
            Spot::post("Qc Qh", "Kh 7h 2c 9s")
                .pot(3666, 10578)
                .stacks(200, 7026),
            &neutral,
            &LEGAL_FULL,
            &CALL_HEAVY,
            "Underpair QQ on K-high turn facing 18 BB barrel committing >50% stack must fold",
        ),
        (
            Spot::post("6d 7h", "3s Kh 4c Js")
                .pot(1247, 3157)
                .stacks(100, 9000),
            &neutral,
            &LEGAL_FULL,
            &CALL_HEAVY,
            "6d 7h gutshot must fold on turn facing 40% pot bet in subgame",
        ),
        (
            Spot::post("7s 2c", "Jd 2d 9c Kh")
                .pot(3600, 6000)
                .stacks(200, 5000),
            &overbet,
            &LEGAL_FULL,
            &CALL_HEAVY,
            "7s 2c bottom pair must fold to 18 BB turn raise even vs overbet bluffer",
        ),
        (
            Spot::post("Jd 2c", "3d Js 7d Kd")
                .pot(4040, 7160)
                .stacks(400, 4040)
                .opp(Some(0)),
            &overbet,
            &LEGAL_FULL,
            &CALL_HEAVY,
            "Below-top-pair on 3-flush turn must fold to overbet jam even vs overbet bluffer",
        ),
        (
            Spot::post("Js 7c", "Ah 9d 6d 7h")
                .pot(4797, 7995)
                .stacks(150, 5451),
            &overbet,
            &LEGAL_SHOVE,
            &CALL_HEAVY,
            "3rd pair Js 7c facing 32 BB Turn raise must fold even vs overbet bluffer",
        ),
        (
            Spot::post("2s 2c", "4h 6c Jd Qc")
                .pot(1200, 2000)
                .stacks(150, 2125),
            &overbet,
            &LEGAL_FULL,
            &CALL_HEAVY_90,
            "Bare underpair 22 under 4 overcards facing 56% stack turn overbet must fold",
        ),
        (
            Spot::post("5d 6d", "Ad 6c 8c Kh")
                .pot(8233, 22393)
                .stacks(300, 8233)
                .opp(Some(0)),
            &maniac,
            &LEGAL_FULL,
            &CALL_HEAVY,
            "Bottom pair must fold to turn all-in at 26.9% pot odds",
        ),
    ];

    for (spot, tracker, legal, probs, msg) in cases {
        let (act, tr) = spot.subgame().once(tracker, probs, legal, &mut rng);
        let reasons = tr.guard_reasons;
        assert_eq!(act, FOLD, "{msg} (reasons: {reasons:?})");
    }
}

#[test]
fn test_subgame_turn_caps_and_pot_control() {
    type CapCase<'a> = (Spot, &'a OpponentTracker, &'a [u8], &'a [f64], u8, &'a str);
    const RAW_ALLIN: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.05, 0.0, 0.05, 0.0, 0.90];
    const SHOVE_HEAVY: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.10, 0.0, 0.0, 0.25, 0.60];
    const BET_HEAVY: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.10, 0.10, 0.40, 0.40, 0.0];
    const RAISE_HEAVY: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.20, 0.30, 0.30, 0.20, 0.0];
    const OVERBET_HEAVY: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.15, 0.05, 0.05, 0.75, 0.0];
    const JAM_HEAVY: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.25, 0.25, 0.0, 0.0, 0.50];

    let neutral = OpponentTracker::new();
    let overbet = make_overbet_bluffer();
    let mut rng = SmallRng::seed_from_u64(20_261_121);

    /* Extreme >20x pot Turn flush ALL_IN must be capped into standard value sizing */
    let (act_flush, _) = Spot::post("4d Td", "Ad 3c 9d 7d")
        .pot(0, 400)
        .stacks(100, 9300)
        .subgame()
        .once(&neutral, &RAW_ALLIN, &LEGAL_OPEN, &mut rng);
    assert_ne!(
        act_flush, ALL_IN,
        "Extreme >20x pot Turn flush ALL_IN must be capped into standard value sizing"
    );

    let bottom_two = Spot::post("3c 4c", "4s Ah Jc 3h")
        .pot(1574, 3411)
        .stacks(100, 9000)
        .subgame();
    assert_eq!(bottom_two.bucket, 58);

    /* bound: CALL_CHECK -> eq CALL_CHECK; ALL_IN -> ne ALL_IN; otherwise < bound */
    let cases: Vec<CapCase<'_>> = vec![
        (
            Spot::post("6h 6c", "2c 3d 9d Kc")
                .pot(1200, 3600)
                .stacks(400, 7218)
                .opp(Some(8000)),
            &neutral,
            &LEGAL_FULL,
            &SHOVE_HEAVY,
            RAISE_MIN,
            "Turn underpair facing 3 BB bet must never raise or shove",
        ),
        (
            Spot::post("Th 9d", "3h As 9s Jc")
                .pot(0, 3000)
                .stacks(200, 12000),
            &neutral,
            &LEGAL_OPEN,
            &BET_HEAVY,
            CALL_CHECK,
            "Turn middle/weak pair in bloated pot must check instead of leading out",
        ),
        (
            bottom_two,
            &neutral,
            &LEGAL_FULL,
            &RAISE_HEAVY,
            CALL_CHECK,
            "Deep-stack Turn bottom two pair facing raise must flat-call instead of 3-betting",
        ),
        (
            Spot::post("Js 7c", "Ah 9d 6d 7h")
                .pot(0, 800)
                .stacks(150, 6800),
            &overbet,
            &LEGAL_OPEN,
            &OVERBET_HEAVY,
            RAISE_HALF_POT,
            "Weak 3rd pair must never lead 1.5x RAISE_HALF_POT on Turn",
        ),
        (
            Spot::post("7s 8d", "Ts Jd 7d 8h")
                .pot(2033, 3173)
                .stacks(400, 3598),
            &neutral,
            &LEGAL_FULL,
            &JAM_HEAVY,
            RAISE_MIN,
            "Two pair without straight on 4-straight Turn must cap raises",
        ),
        (
            Spot::post("Kc Qs", "Th Jh Ts 3c")
                .pot(0, 3850)
                .stacks(100, 5700),
            &neutral,
            &LEGAL_OPEN,
            &PROBS_OPEN_JAM,
            ALL_IN,
            "Turn OESD on paired board must cap open-jam ALL_IN",
        ),
    ];

    for (spot, tracker, legal, probs, bound, msg) in cases {
        let spot = spot.subgame();
        let bucket = spot.bucket;
        for _ in 0..40 {
            let (act, tr) = spot.once(tracker, probs, legal, &mut rng);
            let reasons = tr.guard_reasons;
            match bound {
                CALL_CHECK => assert_eq!(act, CALL_CHECK, "{msg} (bucket {bucket}, got {act})"),
                ALL_IN => assert_ne!(act, ALL_IN, "{msg} (reasons: {reasons:?})"),
                limit => assert!(act < limit, "{msg} (bucket {bucket}, got {act})"),
            }
        }
    }
}

#[test]
fn test_subgame_turn_value_bet_floors() {
    type FloorCase<'a> = (Spot, &'a [f64], usize, usize, bool, &'a str);
    const PASSIVE_88: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.88, 0.0, 0.12, 0.0, 0.0];
    const PASSIVE_85: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.85, 0.10, 0.05, 0.0, 0.0];
    const BET_HEAVY: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.10, 0.10, 0.45, 0.35, 0.0];

    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_125);

    let cases: Vec<FloorCase<'_>> = vec![
        (
            Spot::post("9s 6c", "6s 9d 5h 2c")
                .pot(0, 2152)
                .stacks(200, 15000)
                .opp(Some(1894)),
            &PASSIVE_88,
            80,
            45,
            false,
            "Top Two Pair at effective SPR 0.88 must value-bet majority of the time",
        ),
        (
            Spot::post("Tc 8d", "Ah 8s 3c 5d")
                .pot(0, 1600)
                .stacks(200, 9000),
            &BET_HEAVY,
            60,
            40,
            false,
            "Clean 8 BB SRP Turn middle pair must be free to bet",
        ),
        (
            Spot::post("Ac 3h", "Qh 6h 3c Ah")
                .pot(0, 1380)
                .stacks(100, 8600),
            &PASSIVE_85,
            80,
            35,
            true,
            "Turn Top Two Pair on 3-flush must shift passive checks into compact bets",
        ),
    ];

    for (spot, probs, trials, min_bets, compact_only, msg) in cases {
        let spot = spot.subgame();
        let (counts, _) = spot.sample(&neutral, probs, &LEGAL_OPEN, trials, &mut rng);
        let bets = if compact_only {
            counts[RAISE_MIN as usize] + counts[RAISE_THIRD_POT as usize]
        } else {
            counts[RAISE_MIN as usize..].iter().sum()
        };
        let bucket = spot.bucket;
        assert!(
            bets >= min_bets,
            "{msg} (bucket {bucket}, got {bets}/{trials})"
        );
    }
}

#[test]
fn test_subgame_turn_draw_and_overbet_bluffer_defense() {
    const DRAW_CALL_PROBS: [f64; HOLDEM_NUM_ACTIONS] = [0.10, 0.85, 0.05, 0.0, 0.0, 0.0];

    let overbet = make_overbet_bluffer();
    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_125);

    /* 1. Turn two pair + OESD vs overbet bluffer bluff-catches instead of folding */
    let paired_draw = Spot::post("Td 9d", "Jh Qc Th Jd")
        .pot(7700, 13700)
        .stacks(300, 8700)
        .subgame();
    let mut calls = 0usize;
    for _ in 0..60 {
        let (act, tr) = paired_draw.once(&overbet, &PROBS_FOLD_HEAVY, &LEGAL_SHOVE, &mut rng);
        let reasons = tr.guard_reasons;
        let bucket = paired_draw.bucket;
        assert!(
            !reasons.contains(GuardId::ShoveDefenseFold)
                && !reasons.contains(GuardId::PostBarrelFold),
            "Paired turn hand (bucket {bucket}) vs overbet bluffer must not force-fold ({reasons:?})"
        );
        if act == CALL_CHECK {
            calls += 1;
        }
    }
    assert!(
        calls >= 35,
        "Turn hand vs overbet bluffer must bluff-catch predominantly (got {calls}/60)"
    );

    /* 2. Flush draw + 2 overcards on 3-flush Turn facing 25% pot odds calls overbet bluffer */
    let (act_kj, tr_kj) = Spot::post("Kh Js", "2s 8s 3s Td")
        .pot(1500, 4500)
        .stacks(150, 7900)
        .subgame()
        .once(&overbet, &DRAW_CALL_PROBS, &LEGAL_FULL, &mut rng);
    let reasons_kj = tr_kj.guard_reasons;
    assert_ne!(
        act_kj, FOLD,
        "Turn flush draw + 2 overcards facing 25% pot odds must not fold (reasons: {reasons_kj:?})"
    );

    /* 3. Turn OESD (1/3-pot probe) and Pair+OESD on 4-straight do not force-fold */
    let draw_cases = [
        (
            Spot::post("8h 6c", "7s Td 9c 2h")
                .pot(410, 1660)
                .stacks(100, 9000),
            GuardId::TurnDrawOddsFold,
            "Turn OESD facing 1/3-pot probe (20% pot odds)",
        ),
        (
            Spot::post("Jd 8h", "6s 8d 9c Th")
                .pot(620, 1870)
                .stacks(100, 1880),
            GuardId::PostAirGutshotGuard,
            "Turn Pair+OESD on 4-straight",
        ),
    ];
    for (spot, forbidden_guard, label) in draw_cases {
        let spot = spot.subgame();
        let bucket = spot.bucket;
        let (counts, reasons) = spot.sample(&neutral, &DRAW_CALL_PROBS, &LEGAL_FULL, 60, &mut rng);
        assert!(
            !reasons.contains(forbidden_guard),
            "{label} (bucket {bucket}) must not trigger {forbidden_guard:?} (reasons: {reasons:?})"
        );
        let draw_calls = counts[CALL_CHECK as usize];
        assert!(
            draw_calls >= 40,
            "{label} (bucket {bucket}) must call predominantly (got {draw_calls}/60)"
        );
    }
}
