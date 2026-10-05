use lil_poker_mccfr::cfr::opponent_model::{OpponentStyle, OpponentTracker};
use lil_poker_mccfr::game::holdem::{
    Card, Rank, Suit, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use rand::SeedableRng;

#[test]
fn test_opponent_model_context_adjustment() {
    let mut tracker = OpponentTracker::new();
    /* Simulate calling station: 15 calls, 1 fold, 1 raise */
    for _ in 0..15 {
        tracker.record_action(1, false);
    }
    tracker.record_action(0, false);
    tracker.record_action(2, false);

    let base_strat = [0.15, 0.35, 0.15, 0.15, 0.10, 0.10];
    let legal = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];

    /* Strong hand (bucket 55) against calling station -> raise probability must increase */
    let adj_strong = tracker.adjust_strategy_with_context(&base_strat, &legal, 55, false);
    let total_raise_base: f64 = base_strat[2..=5].iter().sum();
    let total_raise_adj_strong: f64 = adj_strong[2..=5].iter().sum();
    assert!(
        total_raise_adj_strong > total_raise_base,
        "Against calling station, strong hand should raise more"
    );

    /* Weak hand (bucket 10) against calling station -> bluff raises must decrease */
    let adj_weak = tracker.adjust_strategy_with_context(&base_strat, &legal, 10, false);
    let total_raise_adj_weak: f64 = adj_weak[2..=5].iter().sum();
    assert!(
        total_raise_adj_weak < total_raise_base,
        "Against calling station, weak hand bluffs should be reduced"
    );
}

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
fn test_opponent_tracker_advanced_metrics() {
    let mut tracker = OpponentTracker::new();

    /* Hand 1: Preflop open raise, Flop c-bet faced and folded */
    tracker.record_action_street(RAISE_MIN, 1, false);
    tracker.record_action_street(FOLD, 2, true); /* fold to c-bet */
    tracker.end_hand();

    assert_eq!(tracker.total_hands, 1);
    assert_eq!(tracker.vpip_hands, 1);
    assert_eq!(tracker.pfr_hands, 1);
    assert_eq!(tracker.saw_flop, 1);
    assert_eq!(tracker.flop_cbet_faced, 1);
    assert_eq!(tracker.flop_fold_to_cbet, 1);

    /* Hand 2: Preflop limp/call, Flop call c-bet, Showdown reached */
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
    let mut tracker = OpponentTracker::new();

    /* Simulate a Calling Station: VPIP 100%, almost zero folds, high calls */
    for _ in 0..10 {
        tracker.record_action_street(CALL_CHECK, 1, false);
        tracker.record_action_street(CALL_CHECK, 2, false);
        tracker.end_hand();
    }
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
    tracker.record_showdown_hand(opp_trash, &board, true); // opponent raised river with air!

    assert_eq!(tracker.showdown_hands, 1);
    assert_eq!(tracker.showdown_bluff_count, 1);
    assert_eq!(tracker.showdown_bluff_rate(), 1.0);
}
