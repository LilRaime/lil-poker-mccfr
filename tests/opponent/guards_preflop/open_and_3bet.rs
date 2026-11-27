use super::{pf_bucket, raise_count};
use crate::opponent::{
    LEGAL_FULL, LEGAL_SHOVE, PROBS_CALL_HEAVY, PROBS_FOLD_HEAVY, Spot, make_shove_maniac,
    make_ultra_nit,
};
use lil_poker_mccfr::cfr::opponent_model::{GuardId, OpponentTracker, ShortStackRequest};
use lil_poker_mccfr::game::config::{BIG_BLIND, HOLDEM_NUM_ACTIONS, SMALL_BLIND};
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[test]
fn test_preflop_button_steal_requires_unraised_pot() {
    let raw_probs = [0.98, 0.0, 0.02, 0.0, 0.0, 0.0];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN];
    let cases = [
        (
            SMALL_BLIND,
            BIG_BLIND + SMALL_BLIND,
            RAISE_MIN,
            "unraised steal",
        ),
        (
            BIG_BLIND,
            BIG_BLIND * 2,
            FOLD,
            "facing raise must not steal",
        ),
    ];
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(999);
    for (to_call, pot, expected, msg) in cases {
        let s = Spot::bucket_only(1, 20).pot(to_call, pot);
        assert_eq!(
            s.once(&tracker, &raw_probs, &legal, &mut rng).0,
            expected,
            "{msg}"
        );
    }
}

#[test]
fn test_sb_limper_iso_raise_in_bb() {
    let legal_bb = [CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT, RAISE_HALF_POT];
    let passive_pf: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.85, 0.05, 0.05, 0.05, 0.0];
    let mut limper = OpponentTracker::new();
    for _ in 0..5 {
        limper.record_preflop_positional(true, true, false);
        limper.end_hand();
    }
    assert!(limper.sb_limp_ratio() > 0.90);

    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);
    let qto = Spot::preflop("Qs Td").pot(0, 200).stacks(100, 10_000);
    let iso_raises = raise_count(&qto.sample(&limper, &passive_pf, &legal_bb, 100, &mut rng).0);
    let base_raises = raise_count(
        &qto.sample(&neutral, &passive_pf, &legal_bb, 100, &mut rng)
            .0,
    );
    assert!(
        iso_raises >= 35 && iso_raises > base_raises + 15,
        "BB ISO ({iso_raises}/100) vs SB limper must beat baseline ({base_raises}/100)"
    );

    /* Tier 6 playable hands vs >30% SB limper must not be zeroed by PF_DISCIPLINE_GUARD */
    for cards in ["Kd 7c", "Ac 3h", "7h 6c"] {
        let s = Spot::preflop(cards).pot(0, 200).stacks(100, 10_000);
        let t6_iso = raise_count(&s.sample(&limper, &passive_pf, &legal_bb, 100, &mut rng).0);
        assert!(
            t6_iso >= 20,
            "HU BB Tier 6 {cards} vs limper must ISO ({t6_iso}/100)"
        );
    }
}

#[test]
fn test_high_fold_to_3bet_exploit() {
    let flat_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.25, 0.70, 0.05, 0.0, 0.0, 0.0];
    let mut nit3bet = OpponentTracker::new();
    for _ in 0..4 {
        nit3bet.record_facing_3bet(true);
        nit3bet.end_hand();
    }
    assert!(nit3bet.fold_to_3bet_ratio() > 0.90);

    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);
    let ajs = Spot::preflop("Ah Jh").pot(150, 350).stacks(100, 10_000);
    let three_bets = raise_count(
        &ajs.sample(&nit3bet, &flat_heavy, &LEGAL_FULL, 100, &mut rng)
            .0,
    );
    let base_3b = raise_count(
        &ajs.sample(&neutral, &flat_heavy, &LEGAL_FULL, 100, &mut rng)
            .0,
    );
    assert!(
        three_bets >= 18 && three_bets > base_3b + 10,
        "BB must exploit high fold-to-3bet ({three_bets}/100 vs {base_3b}/100)"
    );
}

#[test]
fn test_deep_99_no_open_jam_and_sb_tier5_open_raise() {
    let jam_heavy = [0.0, 0.05, 0.15, 0.10, 0.10, 0.60];
    let limp_heavy = [0.10, 0.70, 0.20, 0.0, 0.0, 0.0];
    let mixed_half: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.55, 0.05, 0.10, 0.25, 0.0];
    let fold_biased: [f64; HOLDEM_NUM_ACTIONS] = [0.65, 0.20, 0.15, 0.0, 0.0, 0.0];
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(777);

    let s_99 = Spot::preflop("9c 9h")
        .active(5)
        .pot(300, 450)
        .stacks(300, 8938);
    let c_99 = s_99
        .sample(&tracker, &jam_heavy, &LEGAL_FULL, 50, &mut rng)
        .0;
    assert_eq!(
        c_99[ALL_IN as usize], 0,
        "99 at ~30 BB 6-max must never open-jam ALL_IN"
    );

    let s_a8o = Spot::preflop("As 8c").pot(100, 300).stacks(200, 5018);
    let raises = raise_count(
        &s_a8o
            .sample(&tracker, &limp_heavy, &LEGAL_FULL, 100, &mut rng)
            .0,
    );
    assert!(
        raises >= 70,
        "A8o at 25 BB HU SB should open-raise (got {raises}/100)"
    );

    /* Tier 5 suited (Qd 8d) at 20 BB HU SB opens compactly via RAISE_MIN rather than limping or 3.5x */
    let s_qd8d = Spot::preflop("Qd 8d").pot(150, 450).stacks(300, 6000);
    let c_qd8d = s_qd8d
        .sample(&tracker, &mixed_half, &LEGAL_FULL, 100, &mut rng)
        .0;
    let min_raises = c_qd8d[RAISE_MIN as usize];
    let oversized = c_qd8d[RAISE_THIRD_POT as usize] + c_qd8d[RAISE_HALF_POT as usize];
    assert!(
        min_raises >= 85 && oversized == 0,
        "Qd8d at 20 BB HU SB must RAISE_MIN ({min_raises}/100, oversized={oversized})"
    );

    /* Suited Tier 7 (4c 2c) at 11 BB HU SB must delegate cleanly instead of auto-folding and open-raise */
    let ss_42c = ShortStackRequest {
        bucket: pf_bucket("4c 2c"),
        to_call: 200,
        pot: 600,
        table_bb: 400,
        my_chips: 4400,
        legal_actions: &LEGAL_FULL,
        ..Default::default()
    };
    assert!(
        tracker.select_short_stack_preflop_action(&ss_42c).is_none(),
        "Suited Tier 7 at 11 BB HU SB must not auto-fold in short-stack Nash table"
    );
    let s_42c = Spot::preflop("4c 2c").pot(200, 600).stacks(400, 4400);
    let open_42c = raise_count(
        &s_42c
            .sample(&tracker, &fold_biased, &LEGAL_FULL, 100, &mut rng)
            .0,
    );
    assert!(
        open_42c >= 65,
        "Suited 42s at 11 BB HU SB must open (got {open_42c}/100)"
    );

    /* Deep HU SB Tier 6 suited/connected (9c 6c) eliminates passive limping in favor of RAISE_MIN */
    let s_96c = Spot::preflop("9c 6c").pot(50, 150).stacks(100, 6000);
    let open_96c = s_96c
        .sample(&tracker, &limp_heavy, &LEGAL_FULL, 100, &mut rng)
        .0[RAISE_MIN as usize];
    assert!(
        open_96c >= 80,
        "Deep HU SB 96s must RAISE_MIN rather than limp ({open_96c}/100)"
    );
}

#[test]
fn test_ultra_nit_sb_steals_pair_setmine_and_no_4bet_jam() {
    let fold_heavy_sb: [f64; HOLDEM_NUM_ACTIONS] = [0.95, 0.05, 0.0, 0.0, 0.0, 0.0];
    let ultra_nit = make_ultra_nit();
    let mut rng = SmallRng::seed_from_u64(4242);

    for cards in ["8s 3s", "Js 3d"] {
        let s = Spot::preflop(cards).pot(50, 150).stacks(100, 9950);
        let steals = s
            .sample(&ultra_nit, &fold_heavy_sb, &LEGAL_FULL, 100, &mut rng)
            .0[RAISE_MIN as usize];
        assert!(
            steals >= 55,
            "Against ultra-nit BB, SB {cards} must steal ({steals}/100)"
        );
    }

    let tracker = OpponentTracker::new();
    let s_44 = Spot::preflop("4s 4h").pot(300, 500).stacks(100, 8700);
    let act_44 = s_44
        .once(
            &tracker,
            &[0.80, 0.20, 0.0, 0.0, 0.0, 0.0],
            &LEGAL_FULL,
            &mut rng,
        )
        .0;
    assert_eq!(act_44, CALL_CHECK, "Pocket 44 vs 3 BB iso-raise must call");

    let s_99 = Spot::preflop("9h 9c").pot(1725, 2550).stacks(150, 7800);
    let c_99 = s_99
        .sample(
            &tracker,
            &[0.05, 0.20, 0.0, 0.15, 0.0, 0.60],
            &LEGAL_FULL,
            40,
            &mut rng,
        )
        .0;
    assert_eq!(
        c_99[ALL_IN as usize], 0,
        "99 at 52 BB vs 14 BB 3-bet must never jam ALL_IN"
    );
}

#[test]
fn test_preflop_four_bet_discipline_and_hu_bb_small_open_defense() {
    let probs_56d = [0.05, 0.85, 0.10, 0.0, 0.0, 0.0];
    let fold_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.75, 0.25, 0.0, 0.0, 0.0, 0.0];
    let four_bet_folds = [
        (
            Spot::preflop("5d 6d")
                .active(2)
                .pot(1815, 5265)
                .stacks(300, 13588),
            probs_56d.as_slice(),
            "Deep speculative suited connector facing 4-bet",
        ),
        (
            Spot::preflop("Qh Js").pot(820, 2120).stacks(100, 4650),
            PROBS_CALL_HEAVY.as_slice(),
            "Offsuit QJo facing 8.2 BB non-all-in 4-bet",
        ),
    ];
    let bb_defenses = [
        ("Kd 4c", 50usize, "Tier-7"),
        ("Qd 5c", 50, "Tier-7"),
        ("9d 8h", 45, "connected Tier 6"),
        ("7h 6s", 45, "connected Tier 6"),
    ];
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(7777);

    for (spot, probs, label) in four_bet_folds {
        let (act, tr) = spot.once(&tracker, probs, &LEGAL_FULL, &mut rng);
        assert_eq!(
            act, FOLD,
            "{label} must fold (reasons: {:?})",
            tr.guard_reasons
        );
    }

    for (cards, min_calls, tier) in bb_defenses {
        let s = Spot::preflop(cards).pot(150, 350).stacks(100, 10_000);
        let calls =
            s.sample(&tracker, &fold_heavy, &LEGAL_FULL, 80, &mut rng).0[CALL_CHECK as usize];
        assert!(
            calls >= min_calls,
            "HU BB {tier} {cards} vs 2.5x must defend ({calls}/80)"
        );
    }

    /* Stack-committing preflop 4-bet with 88 (calling leaves SPR < 0.55) must push ALL_IN rather than flat-call OOP */
    let s_88 = Spot::preflop("8d 8c").pot(2679, 6801).stacks(300, 5887);
    let act_88 = s_88
        .once(
            &tracker,
            &[0.05, 0.90, 0.05, 0.0, 0.0, 0.0],
            &LEGAL_FULL,
            &mut rng,
        )
        .0;
    assert_eq!(
        act_88, ALL_IN,
        "88 facing stack-committing 4-bet (SPR < 0.55) must shove ALL_IN"
    );
}

#[test]
fn test_preflop_hu_sb_mid_hands_open_raise_vs_hyper_aggressor() {
    let legal_sb = [FOLD, CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT, ALL_IN];
    let limp_mixed: [f64; HOLDEM_NUM_ACTIONS] = [0.10, 0.70, 0.20, 0.0, 0.0, 0.0];
    let mut hyper_agg = OpponentTracker::new();
    for i in 0..15 {
        hyper_agg.record_action(if i < 12 { RAISE_MIN } else { CALL_CHECK }, true);
        hyper_agg.end_hand();
    }
    let mut rng = SmallRng::seed_from_u64(7777);

    for cards in ["6h 6c", "Kd Jc"] {
        let s = Spot::preflop(cards).pot(50, 150).stacks(100, 3000);
        let raises = raise_count(&s.sample(&hyper_agg, &limp_mixed, &legal_sb, 80, &mut rng).0);
        assert!(
            raises >= 50,
            "HU SB {cards} at 30 BB vs hyper-aggressor must open ({raises}/80)"
        );
    }
}

#[test]
fn test_preflop_non_allin_3bet_fold_and_low_offsuit_bb_discipline() {
    let three_bet_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.25, 0.20, 0.50, 0.0, 0.0];
    let call_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.10, 0.90, 0.0, 0.0, 0.0, 0.0];
    let maniac = make_shove_maniac();
    let mut rng = SmallRng::seed_from_u64(20_261_122);

    /* 1. Weak offsuit Ace (~30 BB) facing a non-all-in 10 BB 3-bet must fold rather than rescue */
    let s_a6o = Spot::preflop("6h Ad")
        .pot(1500, 2175)
        .stacks(150, 4562)
        .opp(Some(13_000));
    let (act_a6o, tr_a6o) = s_a6o.once(
        &maniac,
        &[0.05, 0.90, 0.0, 0.0, 0.0, 0.05],
        &LEGAL_FULL,
        &mut rng,
    );
    assert_eq!(
        act_a6o, FOLD,
        "10 BB 3-bet at 30 BB must fold A6o (reasons: {:?})",
        tr_a6o.guard_reasons
    );

    /* 2. Weak offsuit Ace in BB facing 2x open must not 3-bet bluff */
    let tracker = OpponentTracker::new();
    let s_a7o = Spot::preflop("7c Ah").pot(300, 900).stacks(300, 11_000);
    let c_a7o = s_a7o
        .sample(&tracker, &three_bet_heavy, &LEGAL_FULL, 40, &mut rng)
        .0;
    assert_eq!(
        raise_count(&c_a7o),
        0,
        "Weak offsuit Ace in BB vs 2x open must not 3-bet"
    );

    /* 3. Low offsuit disconnected gappers (86o / 85o) in HU BB facing 2.5x open must fold */
    for cards in ["8h 6c", "8h 5c"] {
        let s_gap = Spot::preflop(cards).pot(450, 1050).stacks(300, 10_000);
        let (act_gap, tr_gap) = s_gap.once(&tracker, &call_heavy, &LEGAL_FULL, &mut rng);
        assert_eq!(
            act_gap, FOLD,
            "Low gapper {cards} vs 2.5x must fold ({:?})",
            tr_gap.guard_reasons
        );
    }
}

#[test]
fn test_small_pocket_pair_no_bloated_3bet_and_no_jam() {
    let policy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.25, 0.20, 0.25, 0.30, 0.0];
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(4242);

    let s_55 = Spot::preflop("5h 5s").pot(350, 1100).stacks(300, 15_000);
    let (counts, reasons) = s_55.sample(&tracker, &policy, &LEGAL_FULL, 100, &mut rng);
    assert_eq!(
        counts[RAISE_THIRD_POT as usize]
            + counts[RAISE_HALF_POT as usize]
            + counts[ALL_IN as usize],
        0,
        "55 facing open raise must suppress 3-bet raise sizes"
    );
    assert!(
        reasons.contains(GuardId::PfDisciplineGuard),
        "PF_DISCIPLINE_GUARD must fire for 55"
    );
    let raise_min = counts[RAISE_MIN as usize];
    assert!(
        raise_min <= 20,
        "55 should rarely 3-bet min-raise (got {raise_min}/100)"
    );

    let s_77 = Spot::bucket_only(1, 8).pot(400, 1000).stacks(200, 0);
    let act_77 = s_77
        .once(
            &tracker,
            &[0.0, 0.10, 0.0, 0.0, 0.0, 0.90],
            &LEGAL_FULL,
            &mut rng,
        )
        .0;
    assert_ne!(
        act_77, ALL_IN,
        "Pocket 77 facing a standard 2x open must not 3-bet jam ALL_IN!"
    );
}

#[test]
fn test_preflop_hu_minraise_defense_zero_limps_and_linear_3bet() {
    let limp_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.70, 0.25, 0.0, 0.0, 0.0];
    let flat_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.90, 0.05, 0.05, 0.0, 0.0];
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(20_261_122);

    /* 1. HU BB vs 2.0x min-raise (pot_odds = 25%): connected Tier 7 (94s, Q5o) must defend predominantly */
    for cards in ["9h 4h", "Qh 5d"] {
        let s = Spot::preflop(cards).pot(300, 900).stacks(300, 5000);
        let calls = s
            .sample(&tracker, &PROBS_FOLD_HEAVY, &LEGAL_FULL, 80, &mut rng)
            .0[CALL_CHECK as usize];
        assert!(
            calls >= 55,
            "HU BB {cards} vs 2.0x min-raise must call frequently ({calls}/80)"
        );
    }

    /* 2. Zero residual HU SB limps at 16 BB (Q6s), 100 BB (A2o), and Tier 6 vs hyper-aggressor */
    for (cards, my_chips, bb) in [("Qc 6c", 6400, 400), ("Ah 2d", 10_000, 100)] {
        let s = Spot::preflop(cards)
            .pot(bb / 2, bb + bb / 2)
            .stacks(bb, my_chips);
        let limps =
            s.sample(&tracker, &limp_heavy, &LEGAL_FULL, 40, &mut rng).0[CALL_CHECK as usize];
        assert_eq!(
            limps,
            0,
            "HU SB {cards} at {} BB must never limp",
            my_chips / bb
        );
    }

    let mut hyper_agg = OpponentTracker::new();
    for _ in 0..14 {
        hyper_agg.record_action_street_detailed(RAISE_MIN, 1, false, 300, 150);
        hyper_agg.record_preflop_positional(true, true, true);
        hyper_agg.end_hand();
    }
    let s_q6s = Spot::preflop("Qc 6c").pot(100, 300).stacks(200, 3800);
    let limps_q6s = s_q6s
        .sample(&hyper_agg, &limp_heavy, &LEGAL_FULL, 40, &mut rng)
        .0[CALL_CHECK as usize];
    assert_eq!(
        limps_q6s, 0,
        "HU SB Tier 6 vs hyper-aggressor must never limp"
    );

    /* 3. HU linear BB 3-bet shift with Tier 2-3 hands vs high-PFR opponent */
    let s_ajs = Spot::preflop("As Js").pot(150, 350).stacks(100, 9000);
    let three_bets = raise_count(
        &s_ajs
            .sample(&hyper_agg, &flat_heavy, &LEGAL_FULL, 80, &mut rng)
            .0,
    );
    assert!(
        three_bets >= 18,
        "HU BB AJs vs >=42% PFR must 3-bet frequently ({three_bets}/80)"
    );

    /* Deep 46 BB 99 facing 7.5 BB 3-bet (to_call = 5 BB) must cap ALL_IN */
    let neutral_opp = OpponentTracker::new();
    let s_99 = Spot::preflop("9s 9h").pot(500, 1000).stacks(100, 4600);
    let c_99 = s_99
        .sample(
            &neutral_opp,
            &[0.05, 0.35, 0.0, 0.15, 0.15, 0.30],
            &LEGAL_FULL,
            80,
            &mut rng,
        )
        .0;
    let jams_99 = c_99[ALL_IN as usize];
    assert_eq!(
        jams_99, 0,
        "46 BB 99 facing 7.5 BB 3-bet must cap ALL_IN (got {jams_99}/80)"
    );

    /* Weak offsuit hands (tier 6 connected 65o & tier 7+ 64o) facing a 3-bet must fold */
    for cards in ["6c 5h", "6c 4h"] {
        let s_trash = Spot::preflop(cards).pot(390, 1140).stacks(150, 8300);
        let act_trash = s_trash
            .once(
                &neutral_opp,
                &[0.10, 0.85, 0.05, 0.0, 0.0, 0.0],
                &LEGAL_FULL,
                &mut rng,
            )
            .0;
        assert_eq!(
            act_trash, FOLD,
            "Offsuit trash {cards} facing a 3-bet must fold (got {act_trash})"
        );
    }
}

#[test]
fn test_tight_bb_fold_to_steal_opens_sb_and_short_commit_no_flat() {
    let limp_heavy = [0.0, 0.90, 0.10, 0.0, 0.0, 0.0];
    let mut rng = SmallRng::seed_from_u64(20_261_125);

    /* 1. Aggressive postflop opponent who over-folds BB (<= 42% BB VPIP) must not trigger SB hyper-aggressor folds */
    let mut tracker = OpponentTracker::new();
    for i in 0..10 {
        let defends_bb = i < 3;
        tracker.record_preflop_positional(false, defends_bb, false);
        tracker.record_action_street(if defends_bb { CALL_CHECK } else { FOLD }, 1, true);
        tracker.record_action_street_detailed(RAISE_HALF_POT, 2, false, 750, 500);
        tracker.end_hand();
    }
    let s_98o = Spot::preflop("9s 8c").pot(100, 300).stacks(200, 6000);
    let raises = raise_count(
        &s_98o
            .sample(&tracker, &limp_heavy, &LEGAL_FULL, 80, &mut rng)
            .0,
    );
    assert!(
        raises >= 50,
        "SB 98o vs tight BB (30% VPIP) must open-raise (got {raises}/80)"
    );

    /* 2. Short-stack (~19 BB) K7s facing 10 BB 3-bet (55% of remaining stack) must fold instead of dead-money flat-calling */
    let s_k7s = Spot::preflop("Ks 7s").pot(3000, 4500).stacks(300, 5380);
    let act_k7s = s_k7s
        .once(
            &OpponentTracker::new(),
            &PROBS_CALL_HEAVY,
            &LEGAL_SHOVE,
            &mut rng,
        )
        .0;
    assert_ne!(
        act_k7s, CALL_CHECK,
        "Short-stack K7s facing 55%-stack 3-bet must never flat-call"
    );
}
