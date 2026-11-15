use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
use lil_poker_mccfr::game::config::{BIG_BLIND, HOLDEM_NUM_ACTIONS, SMALL_BLIND};
use lil_poker_mccfr::game::holdem::{
    Card, Rank, Suit, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use rand::SeedableRng;

#[test]
fn test_select_action_purified_defense() {
    let tracker = OpponentTracker::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);

    /* 1. Preflop trash hand (bucket 130) facing All-in (to_call = 500)
     * Even if raw_probs has some call probability (e.g. 0.40 call, 0.60 fold),
     * purified selection MUST force FOLD! */
    let raw_probs = [0.60, 0.40, 0.0, 0.0, 0.0, 0.0];
    let legal = [FOLD, CALL_CHECK];
    let chosen = tracker.select_action_purified(&raw_probs, &legal, 130, true, 500, &mut rng);
    assert_eq!(chosen, FOLD, "Preflop trash facing shove must fold!");

    /* 2. Preflop monster hand (bucket 5) facing All-in (to_call = 500)
     * Monster hands should call/raise! */
    let raw_probs_monster = [0.05, 0.85, 0.10, 0.0, 0.0, 0.0];
    let chosen_monster =
        tracker.select_action_purified(&raw_probs_monster, &legal, 5, true, 500, &mut rng);
    assert_eq!(
        chosen_monster, CALL_CHECK,
        "Preflop monster facing shove must not fold!"
    );

    /* 3. Noise clamping: tiny 2% probability should be clamped to 0 */
    let raw_probs_noise = [0.02, 0.98];
    let chosen_clean =
        tracker.select_action_purified(&raw_probs_noise, &legal, 50, false, 0, &mut rng);
    assert_eq!(chosen_clean, CALL_CHECK, "Noise should be clamped");
}

#[test]
fn test_preflop_button_steal_requires_unraised_pot() {
    let tracker = OpponentTracker::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(999);

    /* When facing fold-heavy blueprint [0.98, 0.0, 0.02] */
    let raw_probs = [0.98, 0.0, 0.02, 0.0, 0.0, 0.0];
    let legal = [FOLD, CALL_CHECK, RAISE_MIN];

    /* 1. Unraised pot on button: to_call = SMALL_BLIND (50 chips).
     * Playable hand (tier <= 4, e.g. bucket 20).
     * Folds should be shifted into raises (steal)!
     * 0.98 * 0.85 = +0.833 to RAISE_MIN, making raise overwhelmingly dominant. */
    let action_steal = tracker.select_action_purified_context(
        &raw_probs,
        &legal,
        20,
        1,           /* Preflop */
        SMALL_BLIND, /* Small blind to call in unraised pot */
        BIG_BLIND + SMALL_BLIND,
        None,
        None,
        &mut rng,
    );
    assert_eq!(
        action_steal, RAISE_MIN,
        "Unraised pot on button should steal-raise"
    );

    /* 2. Facing a raise: to_call = BIG_BLIND (100 chips).
     * The bot is in the Big Blind facing a min-raise.
     * This is NOT a button steal situation! Folds should NOT be converted into 3-bets!
     * 0.02 raise noise gets clamped to 0, leaving 100% FOLD. */
    let action_defend = tracker.select_action_purified_context(
        &raw_probs,
        &legal,
        20,
        1,         /* Preflop */
        BIG_BLIND, /* Facing a min-raise (BIG_BLIND to call) */
        BIG_BLIND * 2,
        None,
        None,
        &mut rng,
    );
    assert_eq!(
        action_defend, FOLD,
        "Facing a raise (to_call = BIG_BLIND) must NOT trigger button steal redistribution into raises"
    );
}

#[test]
fn test_sb_limper_iso_raise_and_high_fold_to_3bet_3bet() {
    /* 1. Serial SB limper: in BB facing limp (to_call == 0) with medium-decent hand (QTo) */
    let mut limper = OpponentTracker::new();
    for _ in 0..5 {
        limper.record_preflop_positional(true, true, false);
        limper.end_hand();
    }
    assert!(limper.sb_limp_ratio() > 0.90);

    let neutral = OpponentTracker::new();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(42);
    let legal_bb = [CALL_CHECK, RAISE_MIN, RAISE_THIRD_POT, RAISE_HALF_POT];
    let passive_pf: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.85, 0.05, 0.05, 0.05, 0.0];
    let hole_qto = [
        Card::new(Rank::Queen, Suit::Spades),
        Card::new(Rank::Ten, Suit::Diamonds),
    ];
    let pf_bucket_qto =
        lil_poker_mccfr::cfr::abstraction::preflop_bucket(hole_qto[0], hole_qto[1]).0 as usize;

    let mut iso_raises = 0;
    let mut baseline_raises = 0;
    for _ in 0..100 {
        let (act, _) = limper.select_action_purified_traced_with_stack(
            &passive_pf,
            &legal_bb,
            pf_bucket_qto,
            1,
            0,
            200,
            Some(&hole_qto),
            None,
            1,
            100,
            10_000,
            &mut rng,
        );
        if act >= RAISE_MIN {
            iso_raises += 1;
        }
        let (base_act, _) = neutral.select_action_purified_traced_with_stack(
            &passive_pf,
            &legal_bb,
            pf_bucket_qto,
            1,
            0,
            200,
            Some(&hole_qto),
            None,
            1,
            100,
            10_000,
            &mut rng,
        );
        if base_act >= RAISE_MIN {
            baseline_raises += 1;
        }
    }
    assert!(
        iso_raises >= 35 && iso_raises > baseline_raises + 15,
        "In BB facing serial SB limper with QTo (to_call=0), ISO-raise rate ({}/100) must substantially exceed baseline ({}/100)",
        iso_raises,
        baseline_raises
    );

    /* 2. High fold-to-3bet opener facing BB vs 2.5x open (to_call = 150, bb = 100) with Tier 3 hand */
    let mut nit3bet = OpponentTracker::new();
    for _ in 0..4 {
        nit3bet.record_facing_3bet(true);
        nit3bet.end_hand();
    }
    assert!(nit3bet.fold_to_3bet_ratio() > 0.90);

    let hole_ajs = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Hearts),
    ];
    let pf_bucket_ajs =
        lil_poker_mccfr::cfr::abstraction::preflop_bucket(hole_ajs[0], hole_ajs[1]).0 as usize;
    let legal_vs_open = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let flat_heavy: [f64; HOLDEM_NUM_ACTIONS] = [0.25, 0.70, 0.05, 0.0, 0.0, 0.0];

    let mut three_bets = 0;
    let mut base_three_bets = 0;
    for _ in 0..100 {
        let (act, _) = nit3bet.select_action_purified_traced_with_stack(
            &flat_heavy,
            &legal_vs_open,
            pf_bucket_ajs,
            1,
            150,
            350,
            Some(&hole_ajs),
            None,
            1,
            100,
            10_000,
            &mut rng,
        );
        if act >= RAISE_MIN {
            three_bets += 1;
        }
        let (base_act, _) = neutral.select_action_purified_traced_with_stack(
            &flat_heavy,
            &legal_vs_open,
            pf_bucket_ajs,
            1,
            150,
            350,
            Some(&hole_ajs),
            None,
            1,
            100,
            10_000,
            &mut rng,
        );
        if base_act >= RAISE_MIN {
            base_three_bets += 1;
        }
    }
    assert!(
        three_bets >= 18 && three_bets > base_three_bets + 10,
        "Against opponent who folds >90% to 3-bets, BB must shift passive flats/folds into 3-bets with AJs ({}/100 vs baseline {}/100)",
        three_bets,
        base_three_bets
    );
}

#[test]
fn test_observe_blind_level_decay_and_short_stack_15bb_nash_call() {
    use lil_poker_mccfr::cfr::abstraction::preflop_bucket;

    /* 1. Blind level transition decay (BB=100 -> BB=200 after 20 hands):
     * Should softly decay old counters (~0.71 retain) while preserving ratios and invariants. */
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

    /* Same BB does not decay */
    tracker.observe_blind_level(100);
    assert_eq!(tracker.total_hands, 20);

    /* BB doubles 100 -> 200 (ratio = 2.0): effective counters shrink so new-level hands adapt quickly */
    tracker.observe_blind_level(200);
    assert!(
        tracker.total_hands < 20 && tracker.total_hands >= 12,
        "Expected soft decay on 2x blind jump (100 -> 200), got total_hands={}",
        tracker.total_hands
    );
    assert!(tracker.pfr_hands <= tracker.vpip_hands && tracker.vpip_hands <= tracker.total_hands);

    /* 2. At 15 BB facing a 14 BB shove from a maniac/shove_bot, 88 (bucket 6, Nash cap 26 BB vs maniac)
     * must rescue from blueprint fold and CALL instead of hard-folding! */
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
    let bucket_88 = preflop_bucket(hole_88[0], hole_88[1]).0 as usize;
    let legal = [FOLD, CALL_CHECK];
    let fold_heavy_blueprint: [f64; HOLDEM_NUM_ACTIONS] = [0.92, 0.08, 0.0, 0.0, 0.0, 0.0];
    let mut rng = rand::rngs::SmallRng::seed_from_u64(101);
    let mut calls = 0;
    for _ in 0..50 {
        let (act, _) = maniac.select_action_purified_traced_with_stack(
            &fold_heavy_blueprint,
            &legal,
            bucket_88,
            1,
            1400,
            1550,
            Some(&hole_88),
            None,
            1,
            100,
            1400,
            &mut rng,
        );
        if act == CALL_CHECK {
            calls += 1;
        }
    }
    assert!(
        calls >= 45,
        "At 15 BB facing 14 BB shove from maniac, 88 must call via Nash depth rescue (got {}/50)",
        calls
    );

    /* 3. Unified <= 12 BB Push/Fold handles 10 BB shove call (including tier 4 77 at 0.45 pot odds) */
    let legal_facing_shove = [FOLD, CALL_CHECK];
    let (act_10bb_88, reason_88) = maniac
        .select_short_stack_preflop_action(bucket_88, 900, 1100, 1, 100, 900, &legal_facing_shove)
        .expect("<= 12 BB short-stack engine must activate");
    assert_eq!(
        act_10bb_88, CALL_CHECK,
        "88 must call 10 BB shove ({})",
        reason_88
    );

    let bucket_77 = preflop_bucket(
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Clubs),
    )
    .0 as usize;
    let (act_10bb_77, reason_77) = maniac
        .select_short_stack_preflop_action(bucket_77, 900, 1100, 1, 100, 900, &legal_facing_shove)
        .expect("<= 12 BB short-stack engine must activate");
    assert_eq!(
        act_10bb_77, CALL_CHECK,
        "77 must call 10 BB shove ({})",
        reason_77
    );
}
