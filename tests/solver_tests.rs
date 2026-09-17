use lil_poker_mccfr::cfr::abstraction::get_holdem_infoset_key;
use lil_poker_mccfr::cfr::node::InfosetNode;
use lil_poker_mccfr::game::holdem::{
    evaluate_7cards, Card, Rank, Suit, TexasHoldemGame, CALL_CHECK,
};

#[test]
fn test_evaluate_7cards_categories() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. Royal / Straight Flush: A♥ K♥ Q♥ J♥ T♥ 9♣ 2♦ */
    let hole_sf = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::King, Suit::Hearts),
    ];
    let board_sf = [
        card(Rank::Queen, Suit::Hearts),
        card(Rank::Jack, Suit::Hearts),
        card(Rank::Ten, Suit::Hearts),
        card(Rank::Nine, Suit::Clubs),
        card(Rank::Two, Suit::Diamonds),
    ];
    let score_sf = evaluate_7cards(&hole_sf, &board_sf);
    assert_eq!(score_sf >> 32, 8, "Expected Straight Flush category 8");

    /* 2. Four of a kind (Quads): 9♠ 9♥ 9♦ 9♣ K♣ 2♥ 3♦ */
    let hole_quads = [
        card(Rank::Nine, Suit::Spades),
        card(Rank::Nine, Suit::Hearts),
    ];
    let board_quads = [
        card(Rank::Nine, Suit::Diamonds),
        card(Rank::Nine, Suit::Clubs),
        card(Rank::King, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
        card(Rank::Three, Suit::Diamonds),
    ];
    let score_quads = evaluate_7cards(&hole_quads, &board_quads);
    assert_eq!(score_quads >> 32, 7, "Expected Quads category 7");

    /* 3. Full House: K♠ K♥ K♦ 4♣ 4♥ 2♦ 8♣ */
    let hole_fh = [
        card(Rank::King, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let board_fh = [
        card(Rank::King, Suit::Diamonds),
        card(Rank::Four, Suit::Clubs),
        card(Rank::Four, Suit::Hearts),
        card(Rank::Two, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
    ];
    let score_fh = evaluate_7cards(&hole_fh, &board_fh);
    assert_eq!(score_fh >> 32, 6, "Expected Full House category 6");

    /* 4. Flush: A♠ T♠ 8♠ 6♠ 2♠ K♥ Q♦ */
    let hole_fl = [card(Rank::Ace, Suit::Spades), card(Rank::Ten, Suit::Spades)];
    let board_fl = [
        card(Rank::Eight, Suit::Spades),
        card(Rank::Six, Suit::Spades),
        card(Rank::Two, Suit::Spades),
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
    ];
    let score_fl = evaluate_7cards(&hole_fl, &board_fl);
    assert_eq!(score_fl >> 32, 5, "Expected Flush category 5");

    /* 5. Straight (Ace-low A-2-3-4-5): A♠ 2♥ 3♦ 4♣ 5♠ K♥ 8♦ */
    let hole_st = [card(Rank::Ace, Suit::Spades), card(Rank::Two, Suit::Hearts)];
    let board_st = [
        card(Rank::Three, Suit::Diamonds),
        card(Rank::Four, Suit::Clubs),
        card(Rank::Five, Suit::Spades),
        card(Rank::King, Suit::Hearts),
        card(Rank::Eight, Suit::Diamonds),
    ];
    let score_st = evaluate_7cards(&hole_st, &board_st);
    assert_eq!(score_st >> 32, 4, "Expected Straight category 4");
    assert_eq!(score_st & 0xFF, 3, "Expected 5-high straight");

    /* 6. Three of a kind: Q♠ Q♥ Q♦ 9♣ 7♥ 4♦ 2♣ */
    let hole_trips = [
        card(Rank::Queen, Suit::Spades),
        card(Rank::Queen, Suit::Hearts),
    ];
    let board_trips = [
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Nine, Suit::Clubs),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Four, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    ];
    let score_trips = evaluate_7cards(&hole_trips, &board_trips);
    assert_eq!(score_trips >> 32, 3, "Expected Trips category 3");

    /* 7. Two Pair: J♠ J♥ 8♦ 8♣ A♣ 4♦ 2♥ */
    let hole_tp = [
        card(Rank::Jack, Suit::Spades),
        card(Rank::Jack, Suit::Hearts),
    ];
    let board_tp = [
        card(Rank::Eight, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
        card(Rank::Ace, Suit::Clubs),
        card(Rank::Four, Suit::Diamonds),
        card(Rank::Two, Suit::Hearts),
    ];
    let score_tp = evaluate_7cards(&hole_tp, &board_tp);
    assert_eq!(score_tp >> 32, 2, "Expected Two Pair category 2");

    /* 8. One Pair: T♠ T♥ A♦ 8♣ 6♣ 4♦ 2♥ */
    let hole_op = [card(Rank::Ten, Suit::Spades), card(Rank::Ten, Suit::Hearts)];
    let board_op = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
        card(Rank::Six, Suit::Clubs),
        card(Rank::Four, Suit::Diamonds),
        card(Rank::Two, Suit::Hearts),
    ];
    let score_op = evaluate_7cards(&hole_op, &board_op);
    assert_eq!(score_op >> 32, 1, "Expected One Pair category 1");

    /* 9. High Card: A♠ K♥ J♦ 8♣ 6♣ 4♦ 2♥ */
    let hole_hc = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let board_hc = [
        card(Rank::Jack, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
        card(Rank::Six, Suit::Clubs),
        card(Rank::Four, Suit::Diamonds),
        card(Rank::Two, Suit::Hearts),
    ];
    let score_hc = evaluate_7cards(&hole_hc, &board_hc);
    assert_eq!(score_hc >> 32, 0, "Expected High Card category 0");
}

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
fn test_cfr_plus_non_negative_regret_clamping() {
    let node = InfosetNode::new(4);

    /* Add negative regrets */
    node.update_regrets_cfr_plus(&[-10.0, -50.0, -100.0, -5.0]);

    /* Regrets must be floored at 0, so strategy must be uniform 0.25 */
    let strat = node.get_strategy();
    assert_eq!(strat, vec![0.25, 0.25, 0.25, 0.25]);

    /* Now add positive regret for action 1 (+20.0). Because floor was 0 (not -50),
    action 1 must immediately have positive regret! */
    node.update_regrets_cfr_plus(&[0.0, 20.0, 0.0, 0.0]);
    let strat2 = node.get_strategy();
    assert_eq!(
        strat2[1], 1.0,
        "Action 1 should have 100% prob immediately without negative debt!"
    );
}

#[test]
fn test_holdem_infoset_key_consistency() {
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
    let history_dummy = [vec![1u8, 2u8], vec![], vec![], vec![]];
    let key = get_holdem_infoset_key(&hole, &board, 2, &history_dummy);
    assert!(key.starts_with("F:B"), "Key should be for Flop: {}", key);
    assert!(key.ends_with("/"), "History for round 2 was empty");
}

#[test]
fn test_draw_detection_and_equity_buckets() {
    use lil_poker_mccfr::cfr::abstraction::{detect_draws, postflop_equity_bucket};

    let card = |rank, suit| Card { rank, suit };
    /* Flush draw: A♠ 4♠ on K♠ 8♠ 2♦ */
    let hole_fd = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Four, Suit::Spades),
    ];
    let board_fd = [
        card(Rank::King, Suit::Spades),
        card(Rank::Eight, Suit::Spades),
        card(Rank::Two, Suit::Diamonds),
    ];
    let (is_fd, is_sd) = detect_draws(&hole_fd, &board_fd);
    assert!(is_fd, "Should detect flush draw");
    assert!(!is_sd, "Should not detect straight draw");
    let bucket_fd = postflop_equity_bucket(&hole_fd, &board_fd);
    assert!(
        bucket_fd >= 15,
        "Flush draw with Ace should be high tier draw (bucket >= 15), got {}",
        bucket_fd
    );

    /* One Pair AA vs One Pair 22 should NOT have the same bucket! */
    let hole_aa = [card(Rank::Ace, Suit::Hearts), card(Rank::Two, Suit::Clubs)];
    let board_a = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Nine, Suit::Clubs),
    ];
    let hole_22 = [
        card(Rank::Two, Suit::Hearts),
        card(Rank::Three, Suit::Clubs),
    ];
    let board_2 = [
        card(Rank::Two, Suit::Spades),
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Nine, Suit::Clubs),
    ];
    let bucket_aa = postflop_equity_bucket(&hole_aa, &board_a);
    let bucket_22 = postflop_equity_bucket(&hole_22, &board_2);
    assert!(
        bucket_aa > bucket_22,
        "Pair of Aces ({}) must be in higher bucket than Pair of Twos ({})",
        bucket_aa,
        bucket_22
    );
}
