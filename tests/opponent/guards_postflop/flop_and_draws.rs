use super::super::{
    LEGAL_FULL, LEGAL_OPEN, LEGAL_SHOVE, PROBS_RERAISE_HEAVY, Spot, board, make_overbet_bluffer,
};
use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
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

fn raises(counts: &[usize; HOLDEM_NUM_ACTIONS]) -> usize {
    counts[RAISE_MIN as usize..].iter().sum()
}

#[test]
fn test_monotone_flop_and_paired_river_postflop_guards() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(2027);
    let raise_heavy = [0.05, 0.25, 0.20, 0.20, 0.15, 0.15];
    let call_heavy = [0.05, 0.90, 0.05, 0.0, 0.0, 0.0];

    let s1 = board_spot(2, 16, "Jc 9c 2c").pot(400, 800).stacks(200, 0);
    let (act_oesd, _) = s1.once(&tracker, &call_heavy, &LEGAL_FULL, &mut rng);
    assert_eq!(act_oesd, FOLD, "Non-flush OESD on 3-flush flop must fold!");

    let s2 = board_spot(2, 46, "Ks 2s 9c").pot(500, 1000).stacks(200, 0);
    let (act_tp, _) = s2.once(&tracker, &raise_heavy, &LEGAL_FULL, &mut rng);
    assert_eq!(
        act_tp, CALL_CHECK,
        "Bare Top Pair vs flop donk must flat-call!"
    );

    let s3 = board_spot(4, 65, "4d 8c 6h 2d 6s")
        .pot(9562, 13124)
        .stacks(200, 0);
    let (act_trips, _) = s3.once(&tracker, &call_heavy, &LEGAL_SHOVE, &mut rng);
    assert_eq!(
        act_trips, FOLD,
        "Weak trips on paired river must fold to 48 BB shove!"
    );
}

#[test]
fn test_flop_commit_draw_fold() {
    let mut rng = SmallRng::seed_from_u64(4242);
    let tracker = OpponentTracker::new();
    let call_heavy = [0.10, 0.90, 0.0, 0.0, 0.0, 0.0];
    let half_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.15, 0.05, 0.15, 0.55, 0.10];

    let s1 = with_bucket(Spot::post("Kc 5c", "4c 8c 9s"), 21)
        .pot(2250, 3750)
        .stacks(300, 4712);
    let (act_fd, _) = s1.once(&tracker, &call_heavy, &LEGAL_FULL, &mut rng);
    assert_eq!(
        act_fd, FOLD,
        "Bare flush draw facing 1.5x pot flop overbet committing 48% of stack must fold"
    );

    /* Bare OESD on deep stack facing 1.5x pot flop overbet (37.5% pot odds > 34%) must fold */
    let s2 = Spot::post("Qh 9s", "Jd 2s Tc")
        .pot(1200, 2000)
        .stacks(150, 13500);
    let (act_oesd, tr_oesd) = s2.once(&tracker, &call_heavy, &LEGAL_FULL, &mut rng);
    let reasons = tr_oesd.guard_reasons;
    assert_eq!(
        act_oesd, FOLD,
        "Bare OESD on deep stack facing 1.5x pot flop overbet must fold (reasons: {reasons:?})"
    );

    /* Bare draw in bloated 3-bet flop pot must cap oversized c-bet sizing (no half/all-in) */
    let s3 = Spot::post("5h 6h", "8c Ah 7c")
        .pot(0, 2202)
        .stacks(150, 11500);
    let (counts, _) = s3.sample(&tracker, &half_heavy, &LEGAL_OPEN, 40, &mut rng);
    assert!(
        counts[RAISE_HALF_POT as usize] == 0 && counts[ALL_IN as usize] == 0,
        "Bare draw in 3-bet flop pot must not fire oversized 75%+ c-bet (counts: {counts:?})"
    );
}

#[test]
fn test_flop_wet_set_value_floor() {
    let mut rng = SmallRng::seed_from_u64(20_261_119);
    let tracker = OpponentTracker::new();
    let passive: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.85, 0.0, 0.15, 0.0, 0.0];

    let s1 = Spot::post("Jh Js", "5c 3d Jc")
        .pot(0, 1000)
        .stacks(200, 5331);
    let (c1, _) = s1.sample(&tracker, &passive, &LEGAL_OPEN, 80, &mut rng);
    let wet_set_bets = raises(&c1);
    assert!(
        wet_set_bets >= 45,
        "Top Set on wet flop must value-bet majority of the time, got {wet_set_bets}/80"
    );

    /* Ultra-low SPR (0.34) overpair on flop must jam/value-bet instead of checking down */
    let s2 = Spot::post("8d 8c", "3h 7h 4s")
        .pot(0, 9480)
        .stacks(300, 3208);
    let (c2, _) = s2.sample(&tracker, &passive, &LEGAL_OPEN, 80, &mut rng);
    let low_spr_bets = raises(&c2);
    assert!(
        low_spr_bets >= 50,
        "Overpair 88 at SPR 0.34 on flop must push value mostly (got {low_spr_bets}/80)"
    );
}

#[test]
fn test_flop_floats_overbet_bluff_catch_and_polarized_sizing() {
    let mut rng = SmallRng::seed_from_u64(20_261_123);
    let neutral = OpponentTracker::new();
    let overbet = make_overbet_bluffer();
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.10, 0.90, 0.0, 0.0, 0.0, 0.0];
    let fold_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.85, 0.15, 0.0, 0.0, 0.0, 0.0];
    let passive_open = [0.0, 0.85, 0.05, 0.10, 0.0, 0.0];

    /* 1. Single-raised pot flop floats (50%–60% pot, 2.5–3.0 BB) must NOT auto-fold */
    for (h, b, to_call, pot, bb) in [
        ("Ah Kc", "9h 9s Ts", 900, 2400, 300),
        ("5c Jh", "9h 8d 6c", 1200, 3200, 400),
        ("Ks Jh", "7c 6s 4h", 375, 1125, 150),
    ] {
        let spot = Spot::post(h, b).pot(to_call, pot).stacks(bb, bb * 20);
        let bucket = spot.bucket;
        let (act, tr) = spot.once(&neutral, &call_heavy, &LEGAL_FULL, &mut rng);
        let reasons = tr.guard_reasons;
        assert_ne!(
            act, FOLD,
            "Single-raised flop float {h} (bucket {bucket}) must not force-fold (reasons: {reasons:?})"
        );
    }

    /* 2. Flop OVERBET_BLUFF_CATCH vs 150% overbet bluffer: bottom pair (K2s) and OESD (Q9o) */
    for (h, b) in [("2s Ks", "2h 9s 8h"), ("Qh 9s", "Js Td 4d")] {
        let s = Spot::post(h, b).pot(1200, 2000).stacks(150, 6000);
        let (counts, _) = s.sample(&overbet, &fold_heavy, &LEGAL_FULL, 80, &mut rng);
        let calls = counts[CALL_CHECK as usize];
        assert!(
            calls >= 40,
            "Flop bluff-catcher/OESD {h} vs overbet bluffer must call often (got {calls}/80)"
        );
    }

    /* 3. Flop TPTK value check-raise unlock & Turn polarized RAISE_HALF_POT sizing */
    let s_xr = Spot::post("Ac Kh", "Ad 8s 3c")
        .pot(400, 1200)
        .stacks(100, 9000);
    let (c_xr, _) = s_xr.sample(&overbet, &passive_open, &LEGAL_FULL, 80, &mut rng);
    let xrs = raises(&c_xr);
    assert!(
        xrs >= 15,
        "TPTK on dry flop vs aggro opponent must check-raise sometimes (got {xrs}/80)"
    );

    let s_pol = with_bucket(Spot::post("Ac Kh", "Ad 8s 3c 4h"), 70)
        .pot(0, 1000)
        .stacks(100, 8000);
    let (c_pol, _) = s_pol.sample(
        &neutral,
        &[0.0, 0.45, 0.35, 0.10, 0.10, 0.0],
        &LEGAL_OPEN,
        80,
        &mut rng,
    );
    let polarized_bets = c_pol[RAISE_HALF_POT as usize];
    assert!(
        polarized_bets >= 20,
        "Set on dry Turn must shift weight into polarized RAISE_HALF_POT sizing (got {polarized_bets}/80)"
    );

    /* 4. Normal HU opponent (Fold ~20%, Flop FTC 50%) must not suppress IP Flop c-bets */
    let mut normal_hu = OpponentTracker::new();
    for (act, n, facing) in [
        (CALL_CHECK, 10, false),
        (RAISE_MIN, 6, false),
        (FOLD, 3, true),
        (CALL_CHECK, 3, true),
    ] {
        for _ in 0..n {
            normal_hu.record_action_street(act, 2, facing);
        }
    }
    let s_cb = Spot::bucket_only(2, 3).pot(0, 500).stacks(100, 9700);
    let (c_cb, _) = s_cb.sample(
        &normal_hu,
        &[0.0, 0.35, 0.65, 0.0, 0.0, 0.0],
        &LEGAL_OPEN,
        80,
        &mut rng,
    );
    let cbets = raises(&c_cb);
    assert!(
        cbets >= 25,
        "Normal HU opponent with 50% flop FTC must not trigger sticky-caller c-bet suppression (got {cbets}/80)"
    );

    /* 5. Deep Flop TPTK facing a 3-bet must cap ALL_IN */
    let s_tptk = Spot::post("Ah Kh", "Kc 8d Tc")
        .pot(595, 2575)
        .stacks(100, 9010);
    let (c_tptk, _) = s_tptk.sample(
        &neutral,
        &[0.0, 0.50, 0.10, 0.05, 0.05, 0.30],
        &LEGAL_FULL,
        80,
        &mut rng,
    );
    let tptk_jams = c_tptk[ALL_IN as usize];
    assert_eq!(
        tptk_jams, 0,
        "Deep Flop TPTK facing 3-bet must cap ALL_IN (got {tptk_jams}/80)"
    );

    /* 6. Clean two-overcard Flop float facing a 43% pot c-bet (pot_odds = 0.30 <= 0.31) */
    let s_k9 = Spot::post("Kh 9c", "8d 5s 2h")
        .pot(300, 700)
        .stacks(100, 9700);
    let (c_k9, _) = s_k9.sample(&neutral, &fold_heavy, &LEGAL_FULL, 80, &mut rng);
    let k9_calls = c_k9[CALL_CHECK as usize];
    assert!(
        k9_calls >= 8,
        "Clean two-overcard Flop float vs 43% pot c-bet must rescue calls (got {k9_calls}/80)"
    );

    /* 7. Bottom pair facing 1.5x flop overbet from an overbet bluffer rescues bluff-catch calls */
    let s_48 = Spot::post("4s 8s", "4d Jc 5c")
        .pot(600, 1000)
        .stacks(100, 9000);
    let (c_48, _) = s_48.sample(&overbet, &fold_heavy, &LEGAL_FULL, 80, &mut rng);
    let ob_flop_calls = c_48[CALL_CHECK as usize];
    assert!(
        ob_flop_calls >= 30,
        "Bottom pair vs 1.5x flop overbet from overbet bluffer must rescue calls (got {ob_flop_calls}/80)"
    );

    /* 8. 3-bet pot Flop Overpair must value-bet instead of checking back */
    let s_qq = Spot::post("Qd Qs", "3c 6h 3h")
        .pot(0, 2600)
        .stacks(100, 8700);
    let (c_qq, _) = s_qq.sample(&overbet, &passive_open, &LEGAL_OPEN, 80, &mut rng);
    let qq_cbets = raises(&c_qq);
    assert!(
        qq_cbets >= 45,
        "3-bet pot Flop Overpair must bet frequently instead of checking back (got {qq_cbets}/80)"
    );
}

#[test]
fn test_flop_tpwk_vs_overbet_bluffer_caps_raise_and_3bet_jam() {
    let overbet_villain = make_overbet_bluffer();
    let mut rng = SmallRng::seed_from_u64(20_261_125);
    let spot = Spot::post("2h 7h", "4c 3c 7s");
    let bucket = spot.bucket;
    assert_eq!(bucket, 44);

    for (to_call, pot, my_chips, label) in [
        (750, 1250, 9175, "flop overbet check-raise"),
        (2850, 7350, 7125, "flop 3-bet 4-bet jam"),
    ] {
        let s = spot.clone().pot(to_call, pot).stacks(100, my_chips);
        let (counts, _) = s.sample(
            &overbet_villain,
            &PROBS_RERAISE_HEAVY,
            &LEGAL_FULL,
            40,
            &mut rng,
        );
        assert_eq!(
            raises(&counts),
            0,
            "Dry TPWK (bucket {bucket}) facing {label} must cap raises/jams (counts: {counts:?})"
        );
    }
}

#[test]
fn test_flop_43bb_overbet_shove_underpair_fold_and_low_pfr_not_maniac() {
    let overbet_villain = make_overbet_bluffer();
    let mut rng = SmallRng::seed_from_u64(20_261_126);
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.10, 0.90, 0.0, 0.0, 0.0, 0.0];

    /* 1. Underpair 99 on Kh Tc 5s facing 42.8 BB flop shove vs overbet_bluffer must fold */
    let s_99 = Spot::post("9s 9h", "Kh Tc 5s")
        .pot(6424, 9076)
        .stacks(150, 6424)
        .opp(Some(0));
    let (act_99, tr_99) = s_99.once(&overbet_villain, &call_heavy, &LEGAL_SHOVE, &mut rng);
    let reasons = tr_99.guard_reasons;
    assert_eq!(
        act_99, FOLD,
        "Underpair 99 on Kh Tc 5s facing 42.8 BB flop jam must fold even vs overbet bluffer (reasons: {reasons:?})"
    );

    /* 2. High-VPIP / low-PFR loose-passive 3-bettor must NOT qualify as maniac and turns down 25 BB 3rd-pair jam */
    let mut low_pfr_station = OpponentTracker::new();
    for i in 0..14 {
        if i < 2 {
            low_pfr_station.record_action_street_detailed(RAISE_HALF_POT, 1, false, 1300, 550);
            low_pfr_station.record_action_street_detailed(RAISE_THIRD_POT, 2, false, 2250, 2900);
            low_pfr_station.record_action_street_detailed(ALL_IN, 3, false, 5116, 7400);
        } else {
            low_pfr_station.record_action_street_detailed(CALL_CHECK, 1, false, 300, 450);
            low_pfr_station.record_action_street_detailed(CALL_CHECK, 2, false, 400, 600);
        }
        low_pfr_station.end_hand();
    }
    let s_k9s = Spot::post("Kh 9h", "Qs 2d 9c Ad")
        .pot(5116, 12516)
        .stacks(200, 5116)
        .opp(Some(0))
        .subgame();
    let (act_k9s, _) = s_k9s.once(&low_pfr_station, &call_heavy, &LEGAL_SHOVE, &mut rng);
    assert_eq!(
        act_k9s, FOLD,
        "3rd-pair K9s on Q-2-9-A facing 25.6 BB turn shove from 14%-PFR 3-bettor must fold"
    );
}
