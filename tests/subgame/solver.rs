use lil_poker_mccfr::cfr::fallback::get_postflop_fallback_by_bucket;
use lil_poker_mccfr::cfr::subgame::{
    OpponentReachDistribution, ReachRequest, SubgameContext, SubgameRequest, SubgameSolver,
};
use lil_poker_mccfr::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, SMALL_BLIND,
    SUBGAME_BET_PRESSURE, SUBGAME_POT_LG,
};
use lil_poker_mccfr::game::holdem::{Card, Rank, Suit, ALL_52_CARDS};

#[test]
fn test_subgame_solver_with_state() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Spades),
    ];
    let board = [
        card(Rank::Queen, Suit::Hearts),
        card(Rank::Jack, Suit::Hearts),
        card(Rank::Ten, Suit::Clubs),
        card(Rank::Two, Suit::Diamonds),
    ];
    let history_dummy = [vec![1u8, 2u8], vec![1u8], vec![], vec![]];

    let solver = SubgameSolver::new(100);
    let probs = solver.solve(
        &SubgameRequest {
            hole,
            board: &board,
            contributions: [200, 240],
            raises_this_round: 1,
            ..Default::default()
        },
        &history_dummy,
        None,
    );
    assert_eq!(probs.len(), 6);
    let sum: f64 = probs.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-4,
        "Subgame probs must sum to 1.0, got {sum}"
    );
}

#[test]
fn test_subgame_solver_adaptive_budget() {
    let solver = SubgameSolver::new(2000);
    assert!(solver.adaptive);

    /* 1. Small pot on flop -> scaled down for rapid response */
    let small_budget = solver.compute_budget(2, [SMALL_BLIND, SMALL_BLIND], 0);
    assert!(
        small_budget < 2000,
        "Small pot should use less than base budget, got {small_budget}"
    );

    /* 2. Big pot on river -> scaled up for maximum precision */
    let big_river_budget = solver.compute_budget(4, [SUBGAME_POT_LG / 2, SUBGAME_POT_LG / 2], 0);
    assert!(
        big_river_budget > 2000,
        "Big river pot should exceed base budget, got {big_river_budget}"
    );
    assert!(
        big_river_budget >= 4000,
        "Big river pot should get >= 4000 iters, got {big_river_budget}"
    );

    /* 3. Facing bet -> precision boost */
    let facing_bet_budget =
        solver.compute_budget(3, [SUBGAME_BET_PRESSURE, SUBGAME_BET_PRESSURE * 2], 0);
    let passive_budget = solver.compute_budget(3, [SUBGAME_BET_PRESSURE, SUBGAME_BET_PRESSURE], 0);
    assert!(
        facing_bet_budget > passive_budget,
        "Facing bet budget ({facing_bet_budget}) should exceed passive budget ({passive_budget})"
    );

    /* 4. Non-adaptive solver should always return base iterations */
    let fixed_solver = solver.with_adaptive(false);
    assert_eq!(fixed_solver.compute_budget(4, [500, 500], 0), 2000);
    assert_eq!(fixed_solver.compute_budget(2, [10, 10], 0), 2000);
}

#[test]
fn test_subgame_solver_multithreaded() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Spades),
    ];
    let board = [
        card(Rank::Queen, Suit::Hearts),
        card(Rank::Jack, Suit::Hearts),
        card(Rank::Ten, Suit::Clubs),
        card(Rank::Two, Suit::Diamonds),
    ];
    let history_dummy = [vec![1u8, 2u8], vec![1u8], vec![], vec![]];

    let solver = SubgameSolver::new(400).with_threads(4);
    assert_eq!(solver.threads, 4);

    let probs = solver.solve(
        &SubgameRequest {
            hole,
            board: &board,
            contributions: [200, 240],
            raises_this_round: 1,
            ..Default::default()
        },
        &history_dummy,
        None,
    );
    assert_eq!(probs.len(), 6);
    let sum: f64 = probs.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-4,
        "Parallel subgame probs must sum to 1.0, got {sum}"
    );
    /* Nut straight in position facing small bet should have very low fold probability */
    assert!(
        probs[FOLD as usize] < 0.15,
        "Nut straight should rarely fold, got fold prob {}",
        probs[FOLD as usize]
    );
}

#[test]
fn test_subgame_solver_single_threaded() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Spades),
    ];
    let board = [
        card(Rank::Queen, Suit::Hearts),
        card(Rank::Jack, Suit::Hearts),
        card(Rank::Ten, Suit::Clubs),
        card(Rank::Two, Suit::Diamonds),
    ];
    let history_dummy = [vec![1u8, 2u8], vec![1u8], vec![], vec![]];

    /* Explicitly configure single-threaded execution */
    let solver = SubgameSolver::new(400).with_threads(1);
    assert_eq!(solver.threads, 1);

    let probs = solver.solve(
        &SubgameRequest {
            hole,
            board: &board,
            contributions: [200, 240],
            raises_this_round: 1,
            ..Default::default()
        },
        &history_dummy,
        None,
    );
    assert_eq!(probs.len(), 6);
    let sum: f64 = probs.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-4,
        "Single-threaded subgame probs must sum to 1.0, got {sum}"
    );
    assert!(
        probs[FOLD as usize] < 0.15,
        "Nut straight should rarely fold in single-threaded solver, got {}",
        probs[FOLD as usize]
    );
}

#[test]
fn test_bet_size_aware_reach_and_depth_limited_subgame() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts)];
    /* Paired board: 7d 7c 3s */
    let board = [
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Three, Suit::Spades),
    ];
    let remaining: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|&c| !hole.contains(&c) && !board.contains(&c))
        .collect();

    /* Opponent shoves ALL_IN (5) on Flop vs min-bets RAISE_MIN (2) */
    let hist_min = [vec![2u8, 1u8], vec![2u8], vec![], vec![]];
    let hist_shove = [vec![2u8, 1u8], vec![5u8], vec![], vec![]];

    let dist_min = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining,
            board: &board,
            round: 2,
            target_player: Some(0),
            active_player: Some(1),
            ..Default::default()
        },
        &hist_min,
    );
    let dist_shove = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining,
            board: &board,
            round: 2,
            target_player: Some(0),
            active_player: Some(1),
            ..Default::default()
        },
        &hist_shove,
    );

    /* Unimproved King-Ten offsuit on 7-7-3 board must be treated as air (not One Pair),
     * and weighted even lower against an ALL_IN shove than against a min-bet */
    let kt_air = [
        card(Rank::King, Suit::Spades),
        card(Rank::Ten, Suit::Hearts),
    ];
    let idx_kt = dist_min
        .combos
        .iter()
        .position(|&c| {
            (c[0] == kt_air[0] && c[1] == kt_air[1]) || (c[0] == kt_air[1] && c[1] == kt_air[0])
        })
        .expect("KT combo exists");
    assert!(
        dist_shove.weights[idx_kt] < dist_min.weights[idx_kt],
        "All-in shove must polarize air weight lower ({}) than min-bet ({})",
        dist_shove.weights[idx_kt],
        dist_min.weights[idx_kt]
    );

    /* Depth-limited subgame solver with max_depth_streets = 1 */
    let solver = SubgameSolver::new(200).with_max_depth_streets(1);
    assert_eq!(solver.max_depth_streets, 1);
    let probs = solver.solve(
        &SubgameRequest {
            hole,
            board: &board,
            round: 2,
            my_player: 1,
            contributions: [300, 200],
            raises_this_round: 1,
            ..Default::default()
        },
        &hist_min,
        None,
    );
    let sum: f64 = probs.iter().sum();
    assert!((sum - 1.0).abs() < 1e-4);
}

#[test]
fn test_subgame_child_prior_and_symmetric_hero_reach() {
    let legal = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    /* Strong postflop bucket facing zero bet should put zero mass on FOLD and favor betting/checking */
    let strong_prior = get_postflop_fallback_by_bucket(70, 3, 0, 1000, &legal);
    assert_eq!(strong_prior[FOLD as usize], 0.0);
    assert!(
        strong_prior[RAISE_THIRD_POT as usize] + strong_prior[RAISE_HALF_POT as usize]
            > strong_prior[CALL_CHECK as usize],
        "Strong postflop bucket prior must favor value-betting over pure check: {strong_prior:?}"
    );

    /* Weak postflop bucket facing a full pot bet should favor FOLD over ALL_IN */
    let air_prior = get_postflop_fallback_by_bucket(2, 3, 1000, 1000, &legal);
    assert!(
        air_prior[FOLD as usize] > air_prior[ALL_IN as usize] * 5.0,
        "Weak postflop bucket facing a full-pot bet must fold far more than jam: {air_prior:?}"
    );

    /* Now solve a Turn subgame with symmetric 50/50 hero_reach sampling under a modest budget */
    let hero_hole = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Spades),
    ];
    let board = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::King, Suit::Clubs),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Spades),
    ];
    let history = [vec![1u8, 1u8], vec![1u8, 1u8], vec![], vec![]];

    let solver = SubgameSolver::new(80);
    let mut ctx = SubgameContext::new();
    let probs = solver.solve(
        &SubgameRequest {
            hole: hero_hole,
            board: &board,
            contributions: [2000, 2000],
            ..Default::default()
        },
        &history,
        Some(&mut ctx),
    );

    let sum: f64 = probs.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-6,
        "Solver probs must sum to 1.0, got {sum}"
    );
    assert!(
        probs[FOLD as usize] < 0.01
            && probs[RAISE_THIRD_POT as usize] + probs[RAISE_HALF_POT as usize] > 0.45
            && (1..=5usize).map(|i| probs[i]).sum::<f64>() > 0.99,
        "Top two pair checked to on Turn should never fold and lean toward value betting: {probs:?}"
    );
}

#[test]
fn test_subgame_filters_extreme_overbet_allin() {
    let hole_flush = [
        Card::new(Rank::Four, Suit::Diamonds),
        Card::new(Rank::Ten, Suit::Diamonds),
    ];
    let board_flush = [
        Card::new(Rank::Ace, Suit::Diamonds),
        Card::new(Rank::Three, Suit::Clubs),
        Card::new(Rank::Nine, Suit::Diamonds),
        Card::new(Rank::Seven, Suit::Diamonds),
    ];
    let history = [
        vec![RAISE_MIN, CALL_CHECK],
        vec![CALL_CHECK, CALL_CHECK],
        vec![CALL_CHECK],
        vec![],
    ];
    let solver = SubgameSolver::new(200).with_stack_size(10000);
    let probs = solver.solve(
        &SubgameRequest {
            hole: hole_flush,
            board: &board_flush,
            contributions: [200, 200],
            ..Default::default()
        },
        &history,
        None,
    );
    assert!(
        probs[ALL_IN as usize] < 1e-6,
        "SubgameSolver must filter extreme >3x pot ALL_IN shoves from tree/root: {probs:?}"
    );
    assert!(
        probs[RAISE_THIRD_POT as usize] + probs[RAISE_HALF_POT as usize] > 0.30,
        "Turn flush should value-bet with standard sizing instead of checking/shoving: {probs:?}"
    );
}
