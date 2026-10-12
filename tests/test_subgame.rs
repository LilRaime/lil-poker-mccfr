use lil_poker_mccfr::cfr::subgame::SubgameSolver;
use lil_poker_mccfr::game::config::*;
use lil_poker_mccfr::game::holdem::{Card, Rank, Suit};

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
    let probs = solver.solve_with_state(&hole, &board, 3, &history_dummy, 0, [200, 240], 1);
    assert_eq!(probs.len(), 6);
    let sum: f64 = probs.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-4,
        "Subgame probs must sum to 1.0, got {}",
        sum
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
        "Small pot should use less than base budget, got {}",
        small_budget
    );

    /* 2. Big pot on river -> scaled up for maximum precision */
    let big_river_budget = solver.compute_budget(4, [SUBGAME_POT_LG / 2, SUBGAME_POT_LG / 2], 0);
    assert!(
        big_river_budget > 2000,
        "Big river pot should exceed base budget, got {}",
        big_river_budget
    );
    assert!(
        big_river_budget >= 4000,
        "Big river pot should get >= 4000 iters, got {}",
        big_river_budget
    );

    /* 3. Facing bet -> precision boost */
    let facing_bet_budget =
        solver.compute_budget(3, [SUBGAME_BET_PRESSURE, SUBGAME_BET_PRESSURE * 2], 0);
    let passive_budget = solver.compute_budget(3, [SUBGAME_BET_PRESSURE, SUBGAME_BET_PRESSURE], 0);
    assert!(
        facing_bet_budget > passive_budget,
        "Facing bet budget ({}) should exceed passive budget ({})",
        facing_bet_budget,
        passive_budget
    );

    /* 4. Non-adaptive solver should always return base iterations */
    let fixed_solver = solver.with_adaptive(false);
    assert_eq!(fixed_solver.compute_budget(4, [500, 500], 0), 2000);
    assert_eq!(fixed_solver.compute_budget(2, [10, 10], 0), 2000);
}

#[test]
fn test_opponent_reach_distribution() {
    use lil_poker_mccfr::cfr::subgame::OpponentReachDistribution;
    use lil_poker_mccfr::game::holdem::ALL_52_CARDS;
    use rand::SeedableRng;

    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Spades),
    ];
    let board = [
        card(Rank::Queen, Suit::Hearts),
        card(Rank::Jack, Suit::Hearts),
        card(Rank::Ten, Suit::Clubs),
    ];

    let is_used = |c: Card| hole.contains(&c) || board.contains(&c);
    let remaining: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|&c| !is_used(c))
        .collect();

    /* Scenario 1: Opponent raised preflop (strong range) */
    let history_raised = [vec![1u8, 2u8], vec![], vec![], vec![]];
    let dist_raised = OpponentReachDistribution::build(&remaining, &board, 2, &history_raised);
    assert!(dist_raised.effective_combos() > 100);
    assert!(dist_raised.total_weight > 0.0);

    /* Verify CDF reaches 1.0 */
    assert!((dist_raised.cdf.last().unwrap() - 1.0).abs() < 1e-6);

    /* Sampling works without panic */
    let mut rng = rand::rngs::SmallRng::seed_from_u64(12345);
    let sample = dist_raised.sample(&mut rng);
    assert_ne!(sample[0], sample[1]);
    assert!(remaining.contains(&sample[0]));
    assert!(remaining.contains(&sample[1]));

    /* Scenario 2: Opponent checked/limped (wider, weaker range) */
    let history_limped = [vec![1u8, 1u8], vec![], vec![], vec![]];
    let dist_limped = OpponentReachDistribution::build(&remaining, &board, 2, &history_limped);
    assert_eq!(
        dist_limped.effective_combos(),
        dist_raised.effective_combos()
    );

    /* Top combos list */
    let top = dist_raised.top_combos(5);
    assert_eq!(top.len(), 5);
    for i in 0..4 {
        assert!(
            top[i].1 >= top[i + 1].1,
            "Top combos must be sorted descending"
        );
    }
}
