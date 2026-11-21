use lil_poker_mccfr::cfr::abstraction::preflop_bucket;
use lil_poker_mccfr::cfr::opponent_model::{OpponentTracker, PolicyRequest, ShortStackRequest};
use lil_poker_mccfr::game::config::{BIG_BLIND, HOLDEM_NUM_ACTIONS, SMALL_BLIND};
use lil_poker_mccfr::game::holdem::{
    Card, Rank, Suit, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use rand::rngs::SmallRng;
use rand::SeedableRng;

const LEGAL_SB: [u8; 6] = [
    FOLD,
    CALL_CHECK,
    RAISE_MIN,
    RAISE_THIRD_POT,
    RAISE_HALF_POT,
    ALL_IN,
];
const LEGAL_BB: [u8; 4] = [FOLD, CALL_CHECK, RAISE_MIN, ALL_IN];

#[test]
fn test_select_action_purified_defense() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);
    let legal = [FOLD, CALL_CHECK];

    let chosen = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.60, 0.40, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &legal,
            bucket: 130,
            round: 1,
            to_call: 500,
            pot: 1000,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(chosen, FOLD, "Preflop trash facing shove must fold!");

    let chosen_monster = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.05, 0.85, 0.10, 0.0, 0.0, 0.0],
            legal_actions: &legal,
            bucket: 5,
            round: 1,
            to_call: 500,
            pot: 1000,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(chosen_monster, CALL_CHECK, "Preflop monster must not fold!");

    let chosen_clean = tracker.select_action(
        &PolicyRequest {
            raw_probs: &[0.02, 0.98],
            legal_actions: &legal,
            bucket: 50,
            round: 2,
            to_call: 0,
            pot: 200,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(chosen_clean, CALL_CHECK, "Noise should be clamped");
}

#[test]
fn test_preflop_button_steal_requires_unraised_pot() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(999);
    let raw_probs = [0.98, 0.0, 0.02, 0.0, 0.0, 0.0];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN];

    let action_steal = tracker.select_action(
        &PolicyRequest {
            raw_probs: &raw_probs,
            legal_actions: &legal,
            bucket: 20,
            round: 1,
            to_call: SMALL_BLIND,
            pot: BIG_BLIND + SMALL_BLIND,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        action_steal, RAISE_MIN,
        "Unraised pot on button should steal-raise"
    );

    let action_defend = tracker.select_action(
        &PolicyRequest {
            raw_probs: &raw_probs,
            legal_actions: &legal,
            bucket: 20,
            round: 1,
            to_call: BIG_BLIND,
            pot: BIG_BLIND * 2,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        action_defend, FOLD,
        "Facing a raise must NOT trigger button steal redistribution into raises"
    );
}

#[test]
fn test_sb_limper_iso_raise_in_bb() {
    let mut limper = OpponentTracker::new();
    for _ in 0..5 {
        limper.record_preflop_positional(true, true, false);
        limper.end_hand();
    }
    assert!(limper.sb_limp_ratio() > 0.90);

    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);
    let legal_bb = [CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT, RAISE_HALF_POT];
    let passive_pf: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.85, 0.05, 0.05, 0.05, 0.0];
    let hole_qto = [
        Card::new(Rank::Queen, Suit::Spades),
        Card::new(Rank::Ten, Suit::Diamonds),
    ];
    let pf_bucket_qto = preflop_bucket(hole_qto[0], hole_qto[1]).0;

    let mut iso_raises = 0;
    let mut baseline_raises = 0;
    for _ in 0..100 {
        let req_qto = PolicyRequest {
            raw_probs: &passive_pf,
            legal_actions: &legal_bb,
            bucket: pf_bucket_qto,
            round: 1,
            pot: 200,
            hole: Some(&hole_qto),
            my_chips: 10_000,
            ..Default::default()
        };
        if limper.select_action_traced(&req_qto, &mut rng).0 >= RAISE_MIN {
            iso_raises += 1;
        }
        if neutral.select_action_traced(&req_qto, &mut rng).0 >= RAISE_MIN {
            baseline_raises += 1;
        }
    }
    assert!(
        iso_raises >= 35 && iso_raises > baseline_raises + 15,
        "BB ISO-raise rate ({iso_raises}/100) vs serial SB limper must exceed baseline ({baseline_raises}/100)"
    );
}

#[test]
fn test_high_fold_to_3bet_exploit() {
    let mut nit3bet = OpponentTracker::new();
    for _ in 0..4 {
        nit3bet.record_facing_3bet(true);
        nit3bet.end_hand();
    }
    assert!(nit3bet.fold_to_3bet_ratio() > 0.90);

    let neutral = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(42);
    let hole_ajs = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Hearts),
    ];
    let pf_bucket_ajs = preflop_bucket(hole_ajs[0], hole_ajs[1]).0;
    let flat_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.25, 0.70, 0.05, 0.0, 0.0, 0.0];

    let mut three_bets = 0;
    let mut base_three_bets = 0;
    for _ in 0..100 {
        let req_ajs = PolicyRequest {
            raw_probs: &flat_heavy,
            legal_actions: &LEGAL_SB,
            bucket: pf_bucket_ajs,
            round: 1,
            to_call: 150,
            pot: 350,
            hole: Some(&hole_ajs),
            my_chips: 10_000,
            ..Default::default()
        };
        if nit3bet.select_action_traced(&req_ajs, &mut rng).0 >= RAISE_MIN {
            three_bets += 1;
        }
        if neutral.select_action_traced(&req_ajs, &mut rng).0 >= RAISE_MIN {
            base_three_bets += 1;
        }
    }
    assert!(
        three_bets >= 18 && three_bets > base_three_bets + 10,
        "BB must exploit high fold-to-3bet ({three_bets}/100 vs baseline {base_three_bets}/100)"
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
fn test_short_stack_15bb_and_10bb_nash_calls() {
    let mut maniac = OpponentTracker::new();
    for _ in 0..10 {
        maniac.record_action_street_detailed(ALL_IN, 1, false, 1400, 150);
        maniac.record_preflop_positional(true, true, true);
        maniac.end_hand();
    }
    let hole_88 = [
        Card::new(Rank::Eight, Suit::Hearts),
        Card::new(Rank::Eight, Suit::Spades),
    ];
    let bucket_88 = preflop_bucket(hole_88[0], hole_88[1]).0;
    let legal = [FOLD, CALL_CHECK];
    let fold_heavy_blueprint: [f64; HOLDEM_NUM_ACTIONS] = [0.92, 0.08, 0.0, 0.0, 0.0, 0.0];
    let mut rng = SmallRng::seed_from_u64(101);
    let mut calls = 0;
    for _ in 0..50 {
        let (act, _) = maniac.select_action_traced(
            &PolicyRequest {
                raw_probs: &fold_heavy_blueprint,
                legal_actions: &legal,
                bucket: bucket_88,
                round: 1,
                to_call: 1400,
                pot: 1550,
                hole: Some(&hole_88),
                my_chips: 1400,
                ..Default::default()
            },
            &mut rng,
        );
        if act == CALL_CHECK {
            calls += 1;
        }
    }
    assert!(
        calls >= 45,
        "At 15 BB vs maniac, 88 must call shove (got {calls}/50)"
    );

    let (act_88, reason_88) = maniac
        .select_short_stack_preflop_action(&ShortStackRequest {
            bucket: bucket_88,
            to_call: 900,
            pot: 1100,
            my_chips: 900,
            legal_actions: &legal,
            ..Default::default()
        })
        .expect("<= 12 BB short-stack engine must activate");
    assert_eq!(act_88, CALL_CHECK, "88 must call 10 BB shove ({reason_88})");

    let bucket_77 = preflop_bucket(
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Clubs),
    )
    .0;
    let (act_77, reason_77) = maniac
        .select_short_stack_preflop_action(&ShortStackRequest {
            bucket: bucket_77,
            to_call: 900,
            pot: 1100,
            my_chips: 900,
            legal_actions: &legal,
            ..Default::default()
        })
        .expect("<= 12 BB short-stack engine must activate");
    assert_eq!(act_77, CALL_CHECK, "77 must call 10 BB shove ({reason_77})");
}

#[test]
fn test_deep_99_no_open_jam_and_sb_tier5_open_raise() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(777);

    let hole_99 = [
        Card::new(Rank::Nine, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Hearts),
    ];
    let b_99 = preflop_bucket(hole_99[0], hole_99[1]).0;
    let jam_heavy = [0.0, 0.05, 0.15, 0.10, 0.10, 0.60];
    for _ in 0..50 {
        let (act, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &jam_heavy,
                legal_actions: &LEGAL_SB,
                bucket: b_99,
                round: 1,
                to_call: 300,
                pot: 450,
                hole: Some(&hole_99),
                active_opponents: 5,
                table_bb: 300,
                my_chips: 8938,
                ..Default::default()
            },
            &mut rng,
        );
        assert_ne!(
            act, ALL_IN,
            "Pocket 99 at ~30 BB 6-max must never open-jam ALL_IN"
        );
    }

    let hole_a8o = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Eight, Suit::Clubs),
    ];
    let b_a8o = preflop_bucket(hole_a8o[0], hole_a8o[1]).0;
    let limp_heavy = [0.10, 0.70, 0.20, 0.0, 0.0, 0.0];
    let mut raises = 0usize;
    for _ in 0..100 {
        let (act, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &limp_heavy,
                legal_actions: &LEGAL_SB,
                bucket: b_a8o,
                round: 1,
                to_call: 100,
                pot: 300,
                hole: Some(&hole_a8o),
                table_bb: 200,
                my_chips: 5018,
                ..Default::default()
            },
            &mut rng,
        );
        if act >= RAISE_MIN {
            raises += 1;
        }
    }
    assert!(
        raises >= 70,
        "A8o at 25 BB HU SB should open-raise (got {raises}/100)"
    );
}

#[test]
fn test_short_stack_nash_sb_open_pushes() {
    let tracker = OpponentTracker::new();
    let open_push_cases = [
        (
            Rank::Queen,
            Suit::Spades,
            Rank::Five,
            Suit::Hearts,
            600,
            1920,
            "Q5o @ 3.2 BB",
        ),
        (
            Rank::Eight,
            Suit::Clubs,
            Rank::Five,
            Suit::Spades,
            600,
            1620,
            "85o @ 2.7 BB",
        ),
        (
            Rank::Six,
            Suit::Hearts,
            Rank::Two,
            Suit::Clubs,
            600,
            2520,
            "62o @ 4.2 BB",
        ),
        (
            Rank::Queen,
            Suit::Hearts,
            Rank::Two,
            Suit::Spades,
            200,
            1380,
            "Q2o @ 6.9 BB",
        ),
        (
            Rank::Nine,
            Suit::Clubs,
            Rank::Three,
            Suit::Clubs,
            600,
            3840,
            "93s @ 6.4 BB",
        ),
        (
            Rank::King,
            Suit::Diamonds,
            Rank::Six,
            Suit::Spades,
            600,
            5880,
            "K6o @ 9.8 BB",
        ),
    ];
    for (r1, s1, r2, s2, bb, stack, label) in open_push_cases {
        let b = preflop_bucket(Card::new(r1, s1), Card::new(r2, s2)).0;
        let (act, reason) = tracker
            .select_short_stack_preflop_action(&ShortStackRequest {
                bucket: b,
                to_call: bb / 2,
                pot: bb + bb / 2,
                table_bb: bb,
                my_chips: stack,
                legal_actions: &LEGAL_SB,
                ..Default::default()
            })
            .expect("short stack push/fold active");
        assert_eq!(
            act, ALL_IN,
            "{label} must open-shove ALL_IN, got {act} ({reason})"
        );
    }
}

#[test]
fn test_short_stack_nash_bb_min_raise_defends() {
    let tracker = OpponentTracker::new();
    let bb_call_cases = [
        (
            Rank::Four,
            Suit::Clubs,
            Rank::Two,
            Suit::Clubs,
            200,
            1880,
            "42s @ 9.4 BB",
        ),
        (
            Rank::Jack,
            Suit::Spades,
            Rank::Two,
            Suit::Spades,
            200,
            1580,
            "J2s @ 7.9 BB",
        ),
        (
            Rank::Jack,
            Suit::Diamonds,
            Rank::Three,
            Suit::Hearts,
            200,
            1280,
            "J3o @ 6.4 BB",
        ),
    ];
    for (r1, s1, r2, s2, bb, stack, label) in bb_call_cases {
        let b = preflop_bucket(Card::new(r1, s1), Card::new(r2, s2)).0;
        let (act, reason) = tracker
            .select_short_stack_preflop_action(&ShortStackRequest {
                bucket: b,
                to_call: bb,
                pot: bb * 3,
                table_bb: bb,
                my_chips: stack,
                legal_actions: &LEGAL_BB,
                ..Default::default()
            })
            .expect("short stack BB defense active");
        assert_eq!(
            act, CALL_CHECK,
            "{label} vs HU min-raise must call, got {act} ({reason})"
        );
    }
}

#[test]
fn test_ultra_nit_sb_steals_pair_setmine_and_no_4bet_jam() {
    let mut ultra_nit = OpponentTracker::new();
    for i in 0..15 {
        if i == 0 {
            ultra_nit.record_action_street_detailed(RAISE_MIN, 1, false, 200, 150);
        } else if i < 3 {
            ultra_nit.record_action_street_detailed(CALL_CHECK, 1, false, 100, 150);
        } else {
            ultra_nit.record_action_street_detailed(FOLD, 1, true, 0, 200);
        }
        ultra_nit.end_hand();
    }
    let mut rng = SmallRng::seed_from_u64(4242);
    let fold_heavy_sb: [f64; HOLDEM_NUM_ACTIONS] = [0.95, 0.05, 0.0, 0.0, 0.0, 0.0];
    for (c1, c2, label) in [
        (
            Card::new(Rank::Eight, Suit::Spades),
            Card::new(Rank::Three, Suit::Spades),
            "8s3s",
        ),
        (
            Card::new(Rank::Jack, Suit::Spades),
            Card::new(Rank::Three, Suit::Diamonds),
            "Js3d",
        ),
    ] {
        let hole = [c1, c2];
        let b = preflop_bucket(c1, c2).0;
        let mut steals = 0usize;
        for _ in 0..100 {
            let (act, _) = ultra_nit.select_action_traced(
                &PolicyRequest {
                    raw_probs: &fold_heavy_sb,
                    legal_actions: &LEGAL_SB,
                    bucket: b,
                    round: 1,
                    to_call: 50,
                    pot: 150,
                    hole: Some(&hole),
                    my_chips: 9950,
                    ..Default::default()
                },
                &mut rng,
            );
            if act == RAISE_MIN {
                steals += 1;
            }
        }
        assert!(
            steals >= 55,
            "Against ultra-nit BB, SB {label} must steal (got {steals}/100)"
        );
    }

    let tracker = OpponentTracker::new();
    let hole_44 = [
        Card::new(Rank::Four, Suit::Spades),
        Card::new(Rank::Four, Suit::Hearts),
    ];
    let (act_44, _) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &[0.80, 0.20, 0.0, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_SB,
            bucket: preflop_bucket(hole_44[0], hole_44[1]).0,
            round: 1,
            to_call: 300,
            pot: 500,
            hole: Some(&hole_44),
            my_chips: 8700,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(act_44, CALL_CHECK, "Pocket 44 vs 3 BB iso-raise must call");

    let hole_99 = [
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Clubs),
    ];
    let b_99 = preflop_bucket(hole_99[0], hole_99[1]).0;
    for _ in 0..40 {
        let (act_99, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &[0.05, 0.20, 0.0, 0.15, 0.0, 0.60],
                legal_actions: &LEGAL_SB,
                bucket: b_99,
                round: 1,
                to_call: 1725,
                pot: 2550,
                hole: Some(&hole_99),
                table_bb: 150,
                my_chips: 7800,
                ..Default::default()
            },
            &mut rng,
        );
        assert_ne!(
            act_99, ALL_IN,
            "Pocket 99 at 52 BB vs 14 BB 3-bet must never jam ALL_IN"
        );
    }
}

#[test]
fn test_short_stack_effective_nash_push_call_and_sb_limp_elimination() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(4242);

    let a3o_b = preflop_bucket(
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::Three, Suit::Diamonds),
    )
    .0;
    let (act_a3o, _) = tracker
        .select_short_stack_preflop_action(&ShortStackRequest {
            bucket: a3o_b,
            to_call: 100,
            pot: 300,
            table_bb: 200,
            my_chips: 17568,
            opp_chips: Some(2300),
            legal_actions: &LEGAL_SB,
            ..Default::default()
        })
        .expect("effective stack <= 12 BB activates short-stack push/fold");
    assert_eq!(
        act_a3o, ALL_IN,
        "A3o at 12 BB effective stack must open-push ALL_IN"
    );

    let k5s_b = preflop_bucket(
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Five, Suit::Hearts),
    )
    .0;
    let (act_k5s, _) = tracker
        .select_short_stack_preflop_action(&ShortStackRequest {
            bucket: k5s_b,
            to_call: 2869,
            pot: 3669,
            table_bb: 400,
            my_chips: 2869,
            legal_actions: &[FOLD, CALL_CHECK, ALL_IN],
            ..Default::default()
        })
        .expect("K5s at 8.2 BB facing jam");
    assert_eq!(
        act_k5s, CALL_CHECK,
        "K5s at 8.2 BB in BB must call SB shove"
    );

    let k2o_b = preflop_bucket(
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Two, Suit::Clubs),
    )
    .0;
    let (act_k2o, _) = tracker
        .select_short_stack_preflop_action(&ShortStackRequest {
            bucket: k2o_b,
            to_call: 250,
            pot: 750,
            table_bb: 250,
            my_chips: 2020,
            legal_actions: &LEGAL_BB,
            ..Default::default()
        })
        .expect("K2o at 9.1 BB facing min-raise in BB");
    assert_eq!(
        act_k2o, CALL_CHECK,
        "Ks 2c in BB at 9.1 BB must defend vs min-raise"
    );

    let hole_jj = [
        Card::new(Rank::Jack, Suit::Diamonds),
        Card::new(Rank::Jack, Suit::Spades),
    ];
    let b_jj = preflop_bucket(hole_jj[0], hole_jj[1]).0;
    for _ in 0..40 {
        let (act_jj, _) = tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: &[0.0, 0.45, 0.55, 0.0, 0.0, 0.0],
                legal_actions: &LEGAL_SB,
                bucket: b_jj,
                round: 1,
                to_call: 50,
                pot: 150,
                hole: Some(&hole_jj),
                table_bb: 100,
                my_chips: 9697,
                ..Default::default()
            },
            &mut rng,
        );
        assert!(
            act_jj >= RAISE_MIN,
            "Pocket JJ in SB must raise, got {act_jj}"
        );
    }
}

#[test]
fn test_preflop_four_bet_discipline_and_hu_bb_small_open_defense() {
    let tracker = OpponentTracker::new();
    let mut rng = SmallRng::seed_from_u64(7777);

    let hole_56d = [
        Card::new(Rank::Five, Suit::Diamonds),
        Card::new(Rank::Six, Suit::Diamonds),
    ];
    let b_56d = preflop_bucket(hole_56d[0], hole_56d[1]).0;
    let (act_4bet, tr_4bet) = tracker.select_action_traced(
        &PolicyRequest {
            raw_probs: &[0.05, 0.85, 0.10, 0.0, 0.0, 0.0],
            legal_actions: &LEGAL_SB,
            bucket: b_56d,
            round: 1,
            to_call: 1815,
            pot: 5265,
            hole: Some(&hole_56d),
            active_opponents: 2,
            table_bb: 300,
            my_chips: 13588,
            ..Default::default()
        },
        &mut rng,
    );
    assert_eq!(
        act_4bet, FOLD,
        "Deep speculative suited connector facing 4-bet must fold (reasons: {:?})",
        tr_4bet.guard_reasons
    );

    let fold_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.75, 0.25, 0.0, 0.0, 0.0, 0.0];
    for hole in [
        [
            Card::new(Rank::King, Suit::Diamonds),
            Card::new(Rank::Four, Suit::Clubs),
        ],
        [
            Card::new(Rank::Queen, Suit::Diamonds),
            Card::new(Rank::Five, Suit::Clubs),
        ],
    ] {
        let b = preflop_bucket(hole[0], hole[1]).0;
        let mut calls = 0usize;
        for _ in 0..80 {
            let (act, _) = tracker.select_action_traced(
                &PolicyRequest {
                    raw_probs: &fold_heavy,
                    legal_actions: &LEGAL_SB,
                    bucket: b,
                    round: 1,
                    to_call: 150,
                    pot: 350,
                    hole: Some(&hole),
                    active_opponents: 1,
                    table_bb: 100,
                    my_chips: 10000,
                    ..Default::default()
                },
                &mut rng,
            );
            if act == CALL_CHECK {
                calls += 1;
            }
        }
        assert!(
            calls >= 50,
            "HU BB Tier-7 {hole:?} vs 2.5x open must defend (got {calls}/80)"
        );
    }
}

#[test]
fn test_preflop_hu_sb_mid_hands_open_raise_vs_hyper_aggressor() {
    let mut hyper_agg = OpponentTracker::new();
    for i in 0..15 {
        let is_raise = i < 12;
        hyper_agg.record_action(if is_raise { RAISE_MIN } else { CALL_CHECK }, true);
        hyper_agg.end_hand();
    }
    let legal_sb = [FOLD, CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT, ALL_IN];
    let limp_mixed: [f64; HOLDEM_NUM_ACTIONS] = [0.10, 0.70, 0.20, 0.0, 0.0, 0.0];
    let mut rng = SmallRng::seed_from_u64(7777);

    for hole in [
        [
            Card::new(Rank::Six, Suit::Hearts),
            Card::new(Rank::Six, Suit::Clubs),
        ],
        [
            Card::new(Rank::King, Suit::Diamonds),
            Card::new(Rank::Jack, Suit::Clubs),
        ],
    ] {
        let b = preflop_bucket(hole[0], hole[1]).0;
        let mut raises = 0usize;
        for _ in 0..80 {
            let (act, _) = hyper_agg.select_action_traced(
                &PolicyRequest {
                    raw_probs: &limp_mixed,
                    legal_actions: &legal_sb,
                    bucket: b,
                    round: 1,
                    to_call: 50,
                    pot: 150,
                    hole: Some(&hole),
                    active_opponents: 1,
                    table_bb: 100,
                    my_chips: 3000,
                    ..Default::default()
                },
                &mut rng,
            );
            if act >= RAISE_MIN {
                raises += 1;
            }
        }
        assert!(
            raises >= 50,
            "HU SB hand {hole:?} at 30 BB vs hyper-aggressor must open-raise (got {raises}/80)"
        );
    }
}
