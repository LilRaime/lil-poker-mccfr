use lil_poker_mccfr::game::config::{
    BIG_BLIND, PREFLOP_OPEN_MIN_TOTAL, RAISE_HALF_POT_FLOOR_RIVER, RAISE_HALF_POT_FLOOR_TURN,
    RAISE_MIN_AMT, RAISE_THIRD_POT_FLOOR, SMALL_BLIND, STACK_SIZE,
};
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, Card, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, Rank, Suit,
    TexasHoldemGame,
};
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[test]
fn test_holdem_game_actions_and_transitions() {
    let mut rng = SmallRng::seed_from_u64(42);
    let mut game = TexasHoldemGame::new_random(&mut rng);
    assert_eq!(game.round, 1);
    assert_eq!(game.board.len(), 0);

    let legal = game.legal_actions();
    assert!(!legal.is_empty());

    /* Check preflop call */
    game = game.apply_action(CALL_CHECK);
    assert!(!game.is_terminal());

    /* Preflop check finishes round 1, deals flop (3 cards) */
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 2);
    assert_eq!(game.board.len(), 3);

    /* Flop check-check deals turn (4 cards) */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 3);
    assert_eq!(game.board.len(), 4);

    /* Turn check-check deals river (5 cards) */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 4);
    assert_eq!(game.board.len(), 5);

    /* River check-check leads to showdown */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert!(game.is_terminal());
}

#[test]
fn test_holdem_raise_third_pot_and_all_in() {
    let mut rng = SmallRng::seed_from_u64(42);
    let game = TexasHoldemGame::new_random(&mut rng);
    let legal = game.legal_actions();
    assert!(legal.contains(&RAISE_THIRD_POT));
    assert!(legal.contains(&RAISE_HALF_POT));
    assert!(legal.contains(&RAISE_MIN));
    assert!(
        !legal.contains(&ALL_IN),
        "Deep 100 BB unopened preflop must prune >4x pot ALL_IN shoves"
    );

    /* Short-stack 20 BB game (2000 chips) keeps ALL_IN legal from preflop */
    let short_game = TexasHoldemGame::new_random_with_stack(&mut rng, 2000);
    let short_legal = short_game.legal_actions();
    assert!(short_legal.contains(&ALL_IN));

    let g1 = short_game.apply_action(RAISE_THIRD_POT);
    let expected_raise = ((SMALL_BLIND + BIG_BLIND) / 3).max(RAISE_THIRD_POT_FLOOR);
    assert_eq!(
        g1.contributions[0],
        SMALL_BLIND + (BIG_BLIND - SMALL_BLIND) + expected_raise
    );

    let g2 = g1.apply_action(ALL_IN);
    assert_eq!(g2.contributions[1], 2000);

    let g3 = g2.apply_action(CALL_CHECK);
    assert_eq!(g3.contributions[0], 2000);
    assert_eq!(g3.contributions[1], 2000);
    assert_eq!(g3.board.len(), 5);
    assert!(g3.is_terminal());
    let ret = g3.get_returns();
    assert_eq!(ret[0] + ret[1], 0.0);
    assert_eq!(game.stack_size, STACK_SIZE);
}

#[test]
fn test_holdem_new_dealt_duplicate() {
    let card = |rank, suit| Card { rank, suit };
    let hole0 = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts)];
    let hole1 = [
        card(Rank::King, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let runout = [
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Jack, Suit::Clubs),
        card(Rank::Ten, Suit::Hearts),
        card(Rank::Two, Suit::Spades),
        card(Rank::Three, Suit::Diamonds),
    ];

    let g1 = TexasHoldemGame::new_dealt(hole0, hole1, &runout);
    let g2 = TexasHoldemGame::new_dealt(hole1, hole0, &runout);

    assert_eq!(g1.hole[0], hole0);
    assert_eq!(g1.hole[1], hole1);
    assert_eq!(g2.hole[0], hole1);
    assert_eq!(g2.hole[1], hole0);

    /* Transition both through check/calls to river */
    let mut g1_end = g1;
    while !g1_end.is_terminal() {
        g1_end = g1_end.apply_action(CALL_CHECK);
    }
    let mut g2_end = g2;
    while !g2_end.is_terminal() {
        g2_end = g2_end.apply_action(CALL_CHECK);
    }

    /* Both boards must be identical queen-jack-ten-two-three */
    assert_eq!(g1_end.board.len(), 5);
    assert_eq!(g2_end.board.len(), 5);
    for (i, &c) in runout.iter().enumerate() {
        assert_eq!(g1_end.board[i], c);
        assert_eq!(g2_end.board[i], c);
    }

    /* In g1, AA (seat 0) beats KK (seat 1) */
    assert!(g1_end.get_returns()[0] > 0.0);
    /* In g2, AA (seat 1) beats KK (seat 0) */
    assert!(g2_end.get_returns()[1] > 0.0);
    /* Duplicate symmetry: returns sum to zero across identical plays */
    assert_eq!(g1_end.get_returns()[0], g2_end.get_returns()[1]);
}

#[test]
fn test_geometric_street_aware_bet_sizing() {
    let mut rng = SmallRng::seed_from_u64(42);
    let mut game = TexasHoldemGame::new_random(&mut rng);

    /* Preflop: P0 calls, P1 checks -> Flop (round 2). Pot = 2 * BIG_BLIND */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 2);
    assert_eq!(
        game.current_player(),
        1,
        "BB (player 1) must act first postflop"
    );
    let flop_pot = game.contributions[0] + game.contributions[1];
    assert_eq!(flop_pot, BIG_BLIND * 2);

    /* On Flop: P1 checks, P0 checks -> Turn (round 3). */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 3);
    assert_eq!(
        game.current_player(),
        1,
        "BB (player 1) must act first on Turn"
    );

    /* On Turn: P1 applies RAISE_HALF_POT (polarized 150% pot overbet) */
    let turn_pot = game.contributions[0] + game.contributions[1];
    let turn_g = game.apply_action(RAISE_HALF_POT);
    assert_eq!(
        turn_g.contributions[1] - game.contributions[1],
        (turn_pot * 3 / 2).max(RAISE_HALF_POT_FLOOR_TURN),
        "Turn RAISE_HALF_POT must be 150% pot overbet"
    );

    /* P1 and P0 transition to River (round 4) */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 4);
    assert_eq!(
        game.current_player(),
        1,
        "BB (player 1) must act first on River"
    );
    let river_pot = game.contributions[0] + game.contributions[1];

    /* On River: P1 applies RAISE_HALF_POT (polarized 150% pot overbet) */
    let river_g = game.apply_action(RAISE_HALF_POT);
    let river_bet = river_g.contributions[1] - game.contributions[1];
    assert_eq!(
        river_bet,
        (river_pot * 3 / 2).max(RAISE_HALF_POT_FLOOR_RIVER),
        "River RAISE_HALF_POT must be 150% pot overbet"
    );

    /* Preflop 3.5x 3-bet via RAISE_THIRD_POT against 2.5x open (250 -> 875 total) */
    let pf_game = TexasHoldemGame::new_random(&mut rng);
    let pf_open = pf_game.apply_action(RAISE_MIN);
    assert_eq!(pf_open.contributions[0], 250);
    let pf_3bet = pf_open.apply_action(RAISE_THIRD_POT);
    assert_eq!(pf_3bet.contributions[1], 875);
}

#[test]
fn test_preflop_2_5bb_open_raise() {
    let mut rng = SmallRng::seed_from_u64(42);
    let game = TexasHoldemGame::new_random(&mut rng);

    /* Initially P0 = Small Blind (50), P1 = Big Blind (100) */
    assert_eq!(game.contributions[0], SMALL_BLIND);
    assert_eq!(game.contributions[1], BIG_BLIND);

    /* P0 opens with RAISE_MIN -> commits exactly 2.5x BB = 250 chips */
    let g1 = game.apply_action(RAISE_MIN);
    assert_eq!(g1.contributions[0], PREFLOP_OPEN_MIN_TOTAL);
    assert_eq!(g1.contributions[0], 250);

    /* P1 faces 250 chips (diff = 150). P1 3-bets with RAISE_MIN -> 100 + 150 + 200 = 450 chips */
    let g2 = g1.apply_action(RAISE_MIN);
    assert_eq!(g2.contributions[1], 250 + RAISE_MIN_AMT);
}

#[test]
fn test_holdem_game_short_stack_parameterization() {
    /* 15 BB short-stack match: each player starts with 1500 chips */
    let h0 = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Ace, Suit::Hearts),
    ];
    let h1 = [
        Card::new(Rank::King, Suit::Clubs),
        Card::new(Rank::King, Suit::Diamonds),
    ];
    let runout = [
        Card::new(Rank::Two, Suit::Clubs),
        Card::new(Rank::Five, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Spades),
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Three, Suit::Hearts),
    ];

    let game = TexasHoldemGame::new_dealt_with_stack(h0, h1, &runout, 1500);
    assert_eq!(game.stack_size, 1500);
    assert_eq!(game.contributions, [SMALL_BLIND, BIG_BLIND]);

    let shoved = game.apply_action(ALL_IN);
    assert_eq!(
        shoved.contributions[0], 1500,
        "15 BB shove must commit 1500, not 10_000"
    );

    let called = shoved.apply_action(CALL_CHECK);
    assert!(called.terminal);
    assert_eq!(called.contributions, [1500, 1500]);
    assert_eq!(called.returns[0], 1500.0);
    assert_eq!(called.returns[1], -1500.0);
}

#[test]
fn test_overbet_aggro_archetype_and_stack_aware_self_lookup() {
    use lil_poker_mccfr::cfr::abstraction::preflop_bucket;
    use lil_poker_mccfr::sim::{HoldemStrategy, OpponentArchetype, sample_opponent_action};
    use std::collections::HashMap;

    let mut rng = SmallRng::seed_from_u64(777);
    let mut game = TexasHoldemGame::new_random(&mut rng);
    game = game.apply_action(RAISE_MIN);
    game = game.apply_action(CALL_CHECK);
    let cp = game.current_player();

    /* Overbet Aggro should produce legal actions and frequent postflop aggression when checked to */
    let mut raises = 0usize;
    for _ in 0..40 {
        let act =
            sample_opponent_action(OpponentArchetype::OverbetAggro, None, &game, cp, &mut rng);
        assert!(game.legal_actions().contains(&act));
        if act >= RAISE_MIN {
            raises += 1;
        }
    }
    assert!(
        raises >= 30,
        "OVERBET_AGGRO should frequently c-bet/donk flop when checked to, got {raises}/40 raises"
    );

    /* Stack-aware `self` lookup at 15 BB must read `S15|P:...` keys instead of 100 BB keys */
    let h0 = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Spades),
    ];
    let h1 = [
        Card::new(Rank::Queen, Suit::Hearts),
        Card::new(Rank::Jack, Suit::Hearts),
    ];
    let runout = [
        Card::new(Rank::Two, Suit::Clubs),
        Card::new(Rank::Five, Suit::Diamonds),
        Card::new(Rank::Nine, Suit::Clubs),
        Card::new(Rank::Three, Suit::Spades),
        Card::new(Rank::Four, Suit::Hearts),
    ];
    let short_game = TexasHoldemGame::new_dealt_with_stack(h0, h1, &runout, 1500);
    let (_, pf_name) = preflop_bucket(h0[0], h0[1]);
    let mut strat: HoldemStrategy = HashMap::new();
    strat.insert(format!("S15|P:{pf_name}/"), [0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    strat.insert(format!("P:{pf_name}/"), [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);

    let act_self = sample_opponent_action(
        OpponentArchetype::SelfPlay,
        Some(&strat),
        &short_game,
        0,
        &mut rng,
    );
    assert_eq!(
        act_self, ALL_IN,
        "15 BB `self` archetype must read S15| stack-prefixed blueprint branch"
    );
}
