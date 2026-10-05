use lil_poker_mccfr::game::config::*;
use lil_poker_mccfr::game::holdem::{
    Card, Rank, Suit, TexasHoldemGame, ALL_IN, CALL_CHECK, RAISE_HALF_POT, RAISE_MIN,
    RAISE_THIRD_POT,
};

#[test]
fn test_holdem_game_actions_and_transitions() {
    let mut rng = rand::thread_rng();
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
    let mut rng = rand::thread_rng();
    let game = TexasHoldemGame::new_random(&mut rng);
    let legal = game.legal_actions();
    assert!(legal.contains(&RAISE_THIRD_POT));
    assert!(legal.contains(&RAISE_HALF_POT));
    assert!(legal.contains(&RAISE_MIN));
    assert!(legal.contains(&ALL_IN));

    /* Preflop pot is small blind + big blind */
    let g1 = game.apply_action(RAISE_THIRD_POT);
    let expected_raise = ((SMALL_BLIND + BIG_BLIND) / 3).max(RAISE_THIRD_POT_FLOOR);
    assert_eq!(
        g1.contributions[0],
        SMALL_BLIND + (BIG_BLIND - SMALL_BLIND) + expected_raise
    );

    /* P1 goes ALL_IN: P1 contrib becomes stack_limit (STACK_SIZE). */
    let g2 = g1.apply_action(ALL_IN);
    assert_eq!(g2.contributions[1], STACK_SIZE);

    /* P0 calls all-in: both contribs are STACK_SIZE -> round over -> auto deals to river -> showdown! */
    let g3 = g2.apply_action(CALL_CHECK);
    assert_eq!(g3.contributions[0], STACK_SIZE);
    assert_eq!(g3.contributions[1], STACK_SIZE);
    assert_eq!(g3.board.len(), 5);
    assert!(g3.is_terminal());
    let ret = g3.get_returns();
    assert_eq!(ret[0] + ret[1], 0.0);
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
    let mut rng = rand::thread_rng();
    let mut game = TexasHoldemGame::new_random(&mut rng);

    /* Preflop: P0 calls, P1 checks -> Flop (round 2). Pot = 2 * BIG_BLIND */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 2);
    let flop_pot = game.contributions[0] + game.contributions[1];
    assert_eq!(flop_pot, BIG_BLIND * 2);

    /* On Flop: P0 checks, P1 checks -> Turn (round 3). */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 3);

    /* On Turn: P0 applies RAISE_HALF_POT (geometric 75% pot bet) */
    let turn_g = game.apply_action(RAISE_HALF_POT);
    assert!(turn_g.contributions[0] >= game.contributions[0] + RAISE_HALF_POT_FLOOR_TURN);

    /* P0 and P1 transition to River (round 4) */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 4);
    let river_pot = game.contributions[0] + game.contributions[1];

    /* On River: P0 applies RAISE_HALF_POT (full pot bet 100% of pot!) */
    let river_g = game.apply_action(RAISE_HALF_POT);
    let river_bet = river_g.contributions[0] - game.contributions[0];
    assert_eq!(
        river_bet,
        river_pot.max(RAISE_HALF_POT_FLOOR_RIVER),
        "River RAISE_HALF_POT must be 100% full pot bet"
    );
}
