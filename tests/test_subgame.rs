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
