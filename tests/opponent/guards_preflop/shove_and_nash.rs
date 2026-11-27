use super::{assert_short_stack, pf_bucket, raise_count};
use crate::opponent::{
    LEGAL_BB, LEGAL_FULL, LEGAL_SHOVE, PROBS_FOLD_HEAVY, Spot, make_shove_maniac,
};
use lil_poker_mccfr::cfr::opponent_model::{OpponentTracker, ShortStackRequest};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{ALL_IN, CALL_CHECK, FOLD};
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[test]
fn test_select_action_purified_defense() {
    let legal = [FOLD, CALL_CHECK];
    let p_trash = [0.60, 0.40, 0.0, 0.0, 0.0, 0.0];
    let p_monster = [0.05, 0.85, 0.10, 0.0, 0.0, 0.0];
    let p_clean = [0.02, 0.98];
    let cases = [
        (
            Spot::bucket_only(1, 130).pot(500, 1000),
            p_trash.as_slice(),
            FOLD,
            "Preflop trash folds",
        ),
        (
            Spot::bucket_only(1, 5).pot(500, 1000),
            p_monster.as_slice(),
            CALL_CHECK,
            "Monster calls",
        ),
        (
            Spot::bucket_only(2, 50).pot(0, 200),
            p_clean.as_slice(),
            CALL_CHECK,
            "Noise clamped",
        ),
    ];
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);
    for (spot, probs, expected, msg) in cases {
        assert_eq!(
            spot.once(&tracker, probs, &legal, &mut rng).0,
            expected,
            "{msg}"
        );
    }
}

#[test]
fn test_short_stack_15bb_and_10bb_nash_calls() {
    let legal = [FOLD, CALL_CHECK];
    let fold_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.92, 0.08, 0.0, 0.0, 0.0, 0.0];
    let maniac = make_shove_maniac();
    let mut rng = SmallRng::seed_from_u64(101);

    let s_88 = Spot::preflop("8h 8s").pot(1400, 1550).stacks(100, 1400);
    let calls = s_88.sample(&maniac, &fold_heavy, &legal, 50, &mut rng).0[CALL_CHECK as usize];
    assert!(
        calls >= 45,
        "At 15 BB vs maniac, 88 must call shove (got {calls}/50)"
    );

    for (cards, label) in [("8h 8s", "88"), ("7h 7c", "77")] {
        let req = ShortStackRequest {
            bucket: pf_bucket(cards),
            to_call: 900,
            pot: 1100,
            my_chips: 900,
            legal_actions: &legal,
            ..Default::default()
        };
        assert_short_stack(
            &maniac,
            req,
            CALL_CHECK,
            &format!("{label} must call 10 BB shove"),
        );
    }
}

#[test]
fn test_short_stack_nash_sb_open_pushes() {
    let cases = [
        ("Qs 5h", 600, 1920, "Q5o @ 3.2 BB"),
        ("8c 5s", 600, 1620, "85o @ 2.7 BB"),
        ("6h 2c", 600, 2520, "62o @ 4.2 BB"),
        ("Qh 2s", 200, 1380, "Q2o @ 6.9 BB"),
        ("9c 3c", 600, 3840, "93s @ 6.4 BB"),
        ("Kd 6s", 600, 5880, "K6o @ 9.8 BB"),
    ];
    let tracker = OpponentTracker::new();
    for (cards, bb, stack, label) in cases {
        let req = ShortStackRequest {
            bucket: pf_bucket(cards),
            to_call: bb / 2,
            pot: bb + bb / 2,
            table_bb: bb,
            my_chips: stack,
            legal_actions: &LEGAL_FULL,
            ..Default::default()
        };
        assert_short_stack(
            &tracker,
            req,
            ALL_IN,
            &format!("{label} must open-shove ALL_IN"),
        );
    }
}

#[test]
fn test_short_stack_nash_bb_min_raise_defends() {
    let cases = [
        ("4c 2c", 200, 1880, "42s @ 9.4 BB"),
        ("Js 2s", 200, 1580, "J2s @ 7.9 BB"),
        ("Jd 3h", 200, 1280, "J3o @ 6.4 BB"),
        ("Ks 2c", 250, 2020, "K2o @ 9.1 BB"),
    ];
    let tracker = OpponentTracker::new();
    for (cards, bb, stack, label) in cases {
        let req = ShortStackRequest {
            bucket: pf_bucket(cards),
            to_call: bb,
            pot: bb * 3,
            table_bb: bb,
            my_chips: stack,
            legal_actions: &LEGAL_BB,
            ..Default::default()
        };
        assert_short_stack(
            &tracker,
            req,
            CALL_CHECK,
            &format!("{label} vs HU min-raise must call"),
        );
    }
}

#[test]
fn test_short_stack_effective_nash_push_call_and_sb_limp_elimination() {
    let raw_limp = [0.10, 0.80, 0.10, 0.0, 0.0, 0.0];
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(4242);

    assert_short_stack(
        &tracker,
        ShortStackRequest {
            bucket: pf_bucket("Ac 3d"),
            to_call: 100,
            pot: 300,
            table_bb: 200,
            my_chips: 17568,
            opp_chips: Some(2300),
            legal_actions: &LEGAL_FULL,
            ..Default::default()
        },
        ALL_IN,
        "A3o at 12 BB effective stack must open-push ALL_IN",
    );
    assert_short_stack(
        &tracker,
        ShortStackRequest {
            bucket: pf_bucket("Kh 5h"),
            to_call: 2869,
            pot: 3669,
            table_bb: 400,
            my_chips: 2869,
            legal_actions: &LEGAL_SHOVE,
            ..Default::default()
        },
        CALL_CHECK,
        "K5s at 8.2 BB in BB must call SB shove",
    );

    let s_jj = Spot::preflop("Jd Js").pot(50, 150).stacks(100, 9697);
    let c_jj = s_jj
        .sample(
            &tracker,
            &[0.0, 0.45, 0.55, 0.0, 0.0, 0.0],
            &LEGAL_FULL,
            40,
            &mut rng,
        )
        .0;
    assert_eq!(raise_count(&c_jj), 40, "Pocket JJ in SB must always raise");

    let s_t7o = Spot::bucket_only(1, pf_bucket("Th 7c"))
        .pot(50, 150)
        .stacks(100, 9950);
    let c_t7o = s_t7o
        .sample(&tracker, &raw_limp, &LEGAL_FULL, 20, &mut rng)
        .0;
    assert_eq!(
        c_t7o[CALL_CHECK as usize], 0,
        "Tier 7 T7o on 100 BB HU SB must never limp"
    );
}

#[test]
fn test_maniac_preflop_shove_defense() {
    let call_heavy = [0.05, 0.90, 0.05, 0.0, 0.0, 0.0];
    let cases = [
        (
            77,
            5800,
            6200,
            "Offsuit QJo must fold to a 29 BB preflop shove!",
        ),
        (
            34,
            6400,
            6800,
            "Suited A8s must fold to 32 BB preflop shove!",
        ),
        (
            51,
            2663,
            3063,
            "Suited 97s must fold to 13.3 BB preflop shove!",
        ),
    ];
    let maniac = make_shove_maniac();
    let mut rng = SmallRng::seed_from_u64(2027);
    for (bucket, to_call, pot, msg) in cases {
        let s = Spot::bucket_only(1, bucket)
            .pot(to_call, pot)
            .stacks(200, 0);
        assert_eq!(
            s.once(&maniac, &call_heavy, &LEGAL_SHOVE, &mut rng).0,
            FOLD,
            "{msg}"
        );
    }
}

#[test]
fn test_preflop_deep_and_multiway_shove_defense() {
    let legal = [FOLD, CALL_CHECK];
    let cases = [
        (
            Spot::bucket_only(1, 6).pot(4000, 5000),
            [0.20, 0.80, 0.0, 0.0, 0.0, 0.0],
            FOLD,
            "Pocket 88 facing a 40 BB shove must fold!",
        ),
        (
            Spot::bucket_only(1, 0).pot(4000, 5000),
            [0.40, 0.60, 0.0, 0.0, 0.0, 0.0],
            CALL_CHECK,
            "Pocket Aces facing shove must call!",
        ),
        /* Multiway cold 3-bet shove (~19.4 BB over CO open) with KQs (bucket 15) must fold with zero prior reads */
        (
            Spot::bucket_only(1, 15)
                .active(2)
                .pot(15490, 22160)
                .stacks(800, 15490),
            [0.10, 0.90, 0.0, 0.0, 0.0, 0.0],
            FOLD,
            "KQs must fold to a 19.4 BB multiway cold 3-bet squeeze shove with no reads",
        ),
    ];
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);
    for (spot, probs, expected, msg) in cases {
        assert_eq!(
            spot.once(&tracker, &probs, &legal, &mut rng).0,
            expected,
            "{msg}"
        );
    }
}

#[test]
fn test_6max_short_stack_no_open_limp_and_64o_99_nash_bounds() {
    let fold_cases = [
        (
            "5c 4c",
            6000,
            9000,
            4usize,
            6000,
            43915,
            LEGAL_FULL.as_slice(),
            "54s at 7.3 BB MP 6-max must FOLD instead of open-limping",
        ),
        (
            "Td 8d",
            18000,
            57000,
            2usize,
            6000,
            31915,
            LEGAL_SHOVE.as_slice(),
            "T8s facing 56%-stack multiway 3-bet must FOLD",
        ),
    ];
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_126);

    /* 1-2. 6-max <= 12 BB unopened MP and multiway 56%-stack 3-bet must FOLD cleanly */
    for (cards, to_call, pot, active, bb, stack, legal, msg) in fold_cases {
        let req = ShortStackRequest {
            bucket: pf_bucket(cards),
            to_call,
            pot,
            active_opponents: active,
            table_bb: bb,
            my_chips: stack,
            legal_actions: legal,
            ..Default::default()
        };
        assert_short_stack(&tracker, req, FOLD, msg);
    }

    /* 3. Calibrated Nash push (64o @ 7.5 BB SB) and non-maniac 99 shove-call (27.4 BB) both reject calling/shoving */
    let short_sb = tracker.select_short_stack_preflop_action(&ShortStackRequest {
        bucket: pf_bucket("6c 4s"),
        to_call: 300,
        pot: 900,
        table_bb: 600,
        my_chips: 4512,
        legal_actions: &LEGAL_FULL,
        ..Default::default()
    });
    assert_ne!(
        short_sb.map(|x| x.0),
        Some(ALL_IN),
        "64o at 7.5 BB SB must NOT open-shove ALL_IN in calibrated Nash push table"
    );

    let s_99 = Spot::preflop("9c 9h")
        .pot(8223, 11477)
        .stacks(300, 8223)
        .opp(Some(0));
    let act_99 = s_99
        .once(
            &tracker,
            &[0.10, 0.85, 0.0, 0.0, 0.0, 0.05],
            &LEGAL_SHOVE,
            &mut rng,
        )
        .0;
    assert_eq!(
        act_99, FOLD,
        "99 vs non-maniac 35 BB limp-reshove (27.4 BB to call) must fold"
    );

    /* 4. HU BB facing 2.15x open defends playable Tier 8 (Q4o) frequently instead of 98% fold */
    let s_q4o = Spot::preflop("Qs 4h").pot(172, 472).stacks(150, 10_000);
    let q4o_calls = s_q4o
        .sample(&tracker, &PROBS_FOLD_HEAVY, &LEGAL_BB, 80, &mut rng)
        .0[CALL_CHECK as usize];
    assert!(
        q4o_calls >= 35,
        "Q4o in HU BB vs 2.15x open must defend regularly (got {q4o_calls}/80)"
    );
}
