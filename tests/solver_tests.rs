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

#[test]
fn test_fallback_strategy_preflop_and_postflop() {
    use lil_poker_mccfr::cfr::fallback::get_holdem_fallback_strategy;
    use lil_poker_mccfr::game::holdem::{
        ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
    };

    let card = |rank, suit| Card { rank, suit };

    /* 1. Pocket Aces preflop: should NEVER fold, high raise probability */
    let hole_aa = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts)];
    let board_empty = [];
    let legal_facing_bet = [
        FOLD,
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let strat_aa =
        get_holdem_fallback_strategy(&hole_aa, &board_empty, 1, 20, 40, &legal_facing_bet);
    assert_eq!(
        strat_aa[FOLD as usize], 0.0,
        "Pocket Aces should never fold"
    );
    assert!(
        strat_aa[RAISE_HALF_POT as usize]
            + strat_aa[RAISE_THIRD_POT as usize]
            + strat_aa[RAISE_MIN as usize]
            + strat_aa[ALL_IN as usize]
            > 0.50,
        "Pocket Aces should raise frequently"
    );

    /* 2. 7-2 offsuit preflop facing bet: should fold > 90% */
    let hole_72 = [
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    ];
    let strat_72 =
        get_holdem_fallback_strategy(&hole_72, &board_empty, 1, 20, 40, &legal_facing_bet);
    assert!(
        strat_72[FOLD as usize] > 0.90,
        "72o facing bet should fold > 90%, got {}",
        strat_72[FOLD as usize]
    );

    /* 3. Flush on River: should NEVER fold, even facing bet */
    let hole_fl = [card(Rank::Ace, Suit::Spades), card(Rank::Ten, Suit::Spades)];
    let board_fl = [
        card(Rank::Eight, Suit::Spades),
        card(Rank::Six, Suit::Spades),
        card(Rank::Two, Suit::Spades),
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
    ];
    let strat_fl =
        get_holdem_fallback_strategy(&hole_fl, &board_fl, 4, 100, 300, &legal_facing_bet);
    assert_eq!(
        strat_fl[FOLD as usize], 0.0,
        "Nut flush on river should never fold"
    );
    assert!(
        strat_fl[RAISE_HALF_POT as usize]
            + strat_fl[RAISE_THIRD_POT as usize]
            + strat_fl[RAISE_MIN as usize]
            + strat_fl[ALL_IN as usize]
            > 0.50,
        "Nut flush should raise for value"
    );

    /* 4. Complete Air on Flop when check is free: should check > 85%, never fold */
    let board_air = [
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Nine, Suit::Spades),
    ];
    let legal_free = [
        CALL_CHECK,
        RAISE_MIN,
        RAISE_THIRD_POT,
        RAISE_HALF_POT,
        ALL_IN,
    ];
    let strat_air = get_holdem_fallback_strategy(&hole_72, &board_air, 2, 0, 100, &legal_free);
    assert_eq!(strat_air[FOLD as usize], 0.0);
    assert!(
        strat_air[CALL_CHECK as usize] >= 0.85,
        "Checking should be dominant when check is free with air"
    );
}

#[test]
fn test_opponent_model_context_adjustment() {
    use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
    use lil_poker_mccfr::game::holdem::{
        ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
    };

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

    /* Strong hand (bucket 35) against calling station -> raise probability must increase */
    let adj_strong = tracker.adjust_strategy_with_context(&base_strat, &legal, 35, false);
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
fn test_subgame_solver_with_state() {
    use lil_poker_mccfr::cfr::subgame::SubgameSolver;

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
fn test_holdem_raise_third_pot_and_all_in() {
    use lil_poker_mccfr::game::holdem::{
        TexasHoldemGame, ALL_IN, CALL_CHECK, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
    };
    let mut rng = rand::thread_rng();
    let game = TexasHoldemGame::new_random(&mut rng);
    let legal = game.legal_actions();
    assert!(legal.contains(&RAISE_THIRD_POT));
    assert!(legal.contains(&RAISE_HALF_POT));
    assert!(legal.contains(&RAISE_MIN));
    assert!(legal.contains(&ALL_IN));

    /* Preflop pot is small blind (10) + big blind (20) = 30 */
    /* P0 applies RAISE_THIRD_POT: diff is 10, pot is 30, raise_amt is (30/3).max(30) = 30.
    P0 contrib becomes 10 + 10 + 30 = 50. */
    let g1 = game.apply_action(RAISE_THIRD_POT);
    assert_eq!(g1.contributions[0], 50);

    /* P1 goes ALL_IN: P1 contrib becomes stack_limit (1000). */
    let g2 = g1.apply_action(ALL_IN);
    assert_eq!(g2.contributions[1], 1000);

    /* P0 calls all-in: both contribs are 1000 -> round over -> auto deals to river -> showdown! */
    let g3 = g2.apply_action(CALL_CHECK);
    assert_eq!(g3.contributions[0], 1000);
    assert_eq!(g3.contributions[1], 1000);
    assert_eq!(g3.board.len(), 5);
    assert!(g3.is_terminal());
    let ret = g3.get_returns();
    assert_eq!(ret[0] + ret[1], 0.0);
}

#[test]
fn test_select_action_purified_defense() {
    use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
    use lil_poker_mccfr::game::holdem::{CALL_CHECK, FOLD};
    use rand::SeedableRng;

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
fn test_player_draw_contribution() {
    use lil_poker_mccfr::cfr::abstraction::detect_draws;

    let card = |rank, suit| Card { rank, suit };

    /* 1. Board has 4 hearts, but player has NO hearts:
     * Player does NOT have a flush draw! */
    let hole_no_h = [card(Rank::King, Suit::Spades), card(Rank::Nine, Suit::Diamonds)];
    let board_4h = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Five, Suit::Hearts),
        card(Rank::Two, Suit::Hearts),
    ];
    let (is_fd, _) = detect_draws(&hole_no_h, &board_4h);
    assert!(!is_fd, "Player with no hearts must NOT be credited with a flush draw on a 4-heart board");

    /* 2. Board has 3 hearts, player has 1 heart:
     * Player DOES have a flush draw! */
    let hole_1h = [card(Rank::King, Suit::Hearts), card(Rank::Nine, Suit::Diamonds)];
    let board_3h = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Five, Suit::Hearts),
    ];
    let (is_fd2, _) = detect_draws(&hole_1h, &board_3h);
    assert!(is_fd2, "Player with 1 heart on a 3-heart board must have a flush draw");
}

#[test]
fn test_postflop_pair_hierarchy_and_board_pairs() {
    use lil_poker_mccfr::cfr::abstraction::postflop_equity_bucket;

    let card = |rank, suit| Card { rank, suit };

    /* Overpair: Pocket Queens on a Ten-high board */
    let hole_qq = [card(Rank::Queen, Suit::Spades), card(Rank::Queen, Suit::Hearts)];
    let board_t = [
        card(Rank::Ten, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];
    let bucket_qq = postflop_equity_bucket(&hole_qq, &board_t);
    assert_eq!(bucket_qq, 28, "Overpair QQ on T-7-2 should be bucket 28");

    /* Top Pair Top Kicker: A-K on A-7-2 board */
    let hole_ak = [card(Rank::Ace, Suit::Spades), card(Rank::King, Suit::Hearts)];
    let board_a = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];
    let bucket_ak = postflop_equity_bucket(&hole_ak, &board_a);
    assert_eq!(bucket_ak, 27, "TPTK AK on A-7-2 should be bucket 27");

    /* Top Pair Weak Kicker: A-3 on A-7-2 board */
    let hole_a3 = [card(Rank::Ace, Suit::Spades), card(Rank::Three, Suit::Hearts)];
    let bucket_a3 = postflop_equity_bucket(&hole_a3, &board_a);
    assert_eq!(bucket_a3, 25, "Top pair weak kicker should be bucket 25");
    assert!(bucket_ak > bucket_a3, "TPTK must be strictly higher than weak kicker top pair");

    /* Board Pair: Board is A-A-K, player has 7-2 offsuit (0 aces, 0 kings).
     * Player should NOT be treated as having Top Pair Aces! */
    let hole_junk = [card(Rank::Seven, Suit::Spades), card(Rank::Two, Suit::Hearts)];
    let board_aa_k = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Ace, Suit::Clubs),
        card(Rank::King, Suit::Hearts),
    ];
    let bucket_board_pair = postflop_equity_bucket(&hole_junk, &board_aa_k);
    assert!(
        bucket_board_pair < 25,
        "7-2 on A-A-K board must NOT be evaluated as a high pair (expected < 25, got {})",
        bucket_board_pair
    );
}

#[test]
fn test_rich_infoset_key_generation() {
    use lil_poker_mccfr::cfr::abstraction::{get_holdem_infoset_key, get_holdem_infoset_key_rich};

    let card = |rank, suit| Card { rank, suit };
    let hole = [card(Rank::Ace, Suit::Spades), card(Rank::King, Suit::Hearts)];
    let board = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];

    /* History: Preflop raise-call [2, 1], Flop check [1] */
    let history = [vec![2u8, 1u8], vec![1u8], vec![], vec![]];

    let base_key = get_holdem_infoset_key(&hole, &board, 2, &history);
    let rich_key = get_holdem_infoset_key_rich(&hole, &board, 2, &history);

    assert_eq!(base_key, "F:B27/c");
    assert_eq!(rich_key, "P:rc|F:B27/c");
}

#[test]
fn test_holdem_new_dealt_duplicate() {
    use lil_poker_mccfr::game::holdem::{Card, Rank, Suit, TexasHoldemGame, CALL_CHECK};

    let card = |rank, suit| Card { rank, suit };
    let hole0 = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts)];
    let hole1 = [card(Rank::King, Suit::Spades), card(Rank::King, Suit::Hearts)];
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
    for i in 0..5 {
        assert_eq!(g1_end.board[i], runout[i]);
        assert_eq!(g2_end.board[i], runout[i]);
    }

    /* In g1, AA (seat 0) beats KK (seat 1) */
    assert!(g1_end.get_returns()[0] > 0.0);
    /* In g2, AA (seat 1) beats KK (seat 0) */
    assert!(g2_end.get_returns()[1] > 0.0);
    /* Duplicate symmetry: returns sum to zero across identical plays */
    assert_eq!(g1_end.get_returns()[0], g2_end.get_returns()[1]);
}

#[test]
fn test_opponent_tracker_advanced_metrics() {
    use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
    use lil_poker_mccfr::game::holdem::{CALL_CHECK, FOLD, RAISE_MIN};

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
fn test_dcfr_regret_discounting() {
    use lil_poker_mccfr::cfr::node::InfosetNode;

    let node = InfosetNode::new(3);

    /* Initial regrets: action 0 has +10.0, action 1 has -10.0, action 2 has 0.0 */
    node.update_regrets_dcfr(&[10.0, -10.0, 0.0], 1.0, 1.0);
    assert!((node.get_regret(0) - 10.0).abs() < 1e-4);
    assert!((node.get_regret(1) - (-10.0)).abs() < 1e-4);

    /* Apply DCFR step with pos_discount = 0.8, neg_discount = 0.5, delta = [0, 0, 0] */
    node.update_regrets_dcfr(&[0.0, 0.0, 0.0], 0.80, 0.50);
    /* Action 0 was +10.0 -> becomes 10.0 * 0.8 = 8.0 */
    assert!((node.get_regret(0) - 8.0).abs() < 1e-4, "Positive regret discounted by 0.8");
    /* Action 1 was -10.0 -> becomes -10.0 * 0.5 = -5.0 */
    assert!((node.get_regret(1) - (-5.0)).abs() < 1e-4, "Negative regret discounted by 0.5");
}

#[test]
fn test_board_texture_and_redraw_potential() {
    use lil_poker_mccfr::cfr::abstraction::{
        detect_board_texture, postflop_equity_bucket, BoardFlushTexture,
    };
    use lil_poker_mccfr::game::holdem::{Card, Rank, Suit};

    let card = |rank, suit| Card { rank, suit };

    /* 1. Monotone Board */
    let board_mono = [
        card(Rank::King, Suit::Hearts),
        card(Rank::Eight, Suit::Hearts),
        card(Rank::Four, Suit::Hearts),
    ];
    let tex_mono = detect_board_texture(&board_mono);
    assert_eq!(tex_mono.flush_texture, BoardFlushTexture::Monotone);
    assert!(!tex_mono.is_paired);

    /* 2. Paired Connected Board */
    let board_pair = [
        card(Rank::Nine, Suit::Spades),
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Eight, Suit::Diamonds),
    ];
    let tex_pair = detect_board_texture(&board_pair);
    assert!(tex_pair.is_paired);
    assert!(tex_pair.is_connected);

    /* 3. Redraw potential: Top Pair + Flush Draw vs Dry Top Pair */
    let hole_tptk_fd = [card(Rank::Ace, Suit::Hearts), card(Rank::King, Suit::Hearts)];
    let board_two_tone = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Two, Suit::Hearts),
    ];
    let bucket_with_fd = postflop_equity_bucket(&hole_tptk_fd, &board_two_tone);

    let hole_tptk_dry = [card(Rank::Ace, Suit::Spades), card(Rank::King, Suit::Clubs)];
    let bucket_dry = postflop_equity_bucket(&hole_tptk_dry, &board_two_tone);

    assert!(
        bucket_with_fd > bucket_dry,
        "TPTK with Nut Flush Draw ({}) must have higher bucket than dry TPTK ({})",
        bucket_with_fd,
        bucket_dry
    );
}

#[test]
fn test_opponent_style_classification_and_showdown() {
    use lil_poker_mccfr::cfr::opponent_model::{OpponentStyle, OpponentTracker};
    use lil_poker_mccfr::game::holdem::{Card, Rank, Suit, CALL_CHECK};

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
    let opp_trash = [card(Rank::Seven, Suit::Spades), card(Rank::Two, Suit::Clubs)];
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

#[test]
fn test_geometric_street_aware_bet_sizing() {
    use lil_poker_mccfr::game::holdem::{
        TexasHoldemGame, CALL_CHECK, RAISE_HALF_POT,
    };
    let mut rng = rand::thread_rng();
    let mut game = TexasHoldemGame::new_random(&mut rng);

    /* Preflop: P0 calls (20), P1 checks -> Flop (round 2). Pot = 40 */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 2);
    let flop_pot = game.contributions[0] + game.contributions[1];
    assert_eq!(flop_pot, 40);

    /* On Flop: P0 checks, P1 checks -> Turn (round 3). Pot = 40 */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 3);

    /* On Turn: P0 applies RAISE_HALF_POT (geometric 75% pot bet: 40 * 3/4 = 30, min 60 -> raise 60) */
    let turn_g = game.apply_action(RAISE_HALF_POT);
    assert!(turn_g.contributions[0] >= game.contributions[0] + 60);

    /* P0 and P1 transition to River (round 4) */
    game = game.apply_action(CALL_CHECK);
    game = game.apply_action(CALL_CHECK);
    assert_eq!(game.round, 4);
    let river_pot = game.contributions[0] + game.contributions[1];

    /* On River: P0 applies RAISE_HALF_POT (full pot bet 100% of pot!) */
    let river_g = game.apply_action(RAISE_HALF_POT);
    let river_bet = river_g.contributions[0] - game.contributions[0];
    assert_eq!(river_bet, river_pot.max(80), "River RAISE_HALF_POT must be 100% full pot bet");
}

#[test]
fn test_bitwise_evaluate_7cards_tiebreakers() {
    let card = |rank, suit| Card { rank, suit };

    /* 1. Straight: Broadway A-K-Q-J-T vs King-high K-Q-J-T-9 vs Wheel 5-4-3-2-A */
    let hole_broadway = [card(Rank::Ace, Suit::Spades), card(Rank::King, Suit::Hearts)];
    let board_broadway = [
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Jack, Suit::Clubs),
        card(Rank::Ten, Suit::Hearts),
        card(Rank::Four, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];
    let score_broadway = evaluate_7cards(&hole_broadway, &board_broadway);
    assert_eq!(score_broadway >> 32, 4);
    assert_eq!(score_broadway & 0xFF, 12, "Broadway high rank must be 12 (Ace)");

    let hole_wheel = [card(Rank::Ace, Suit::Spades), card(Rank::Two, Suit::Hearts)];
    let board_wheel = [
        card(Rank::Three, Suit::Diamonds),
        card(Rank::Four, Suit::Clubs),
        card(Rank::Five, Suit::Hearts),
        card(Rank::Nine, Suit::Spades),
        card(Rank::Eight, Suit::Clubs),
    ];
    let score_wheel = evaluate_7cards(&hole_wheel, &board_wheel);
    assert_eq!(score_wheel >> 32, 4);
    assert_eq!(score_wheel & 0xFF, 3, "Wheel high rank must be 3 (Five)");
    assert!(score_broadway > score_wheel, "Broadway must beat Wheel");

    /* 2. Flush tiebreakers: A♠ K♠ Q♠ J♠ 9♠ vs A♠ K♠ Q♠ J♠ 8♠ */
    let hole_fl1 = [card(Rank::Ace, Suit::Spades), card(Rank::Nine, Suit::Spades)];
    let hole_fl2 = [card(Rank::Ace, Suit::Spades), card(Rank::Eight, Suit::Spades)];
    let board_fl = [
        card(Rank::King, Suit::Spades),
        card(Rank::Queen, Suit::Spades),
        card(Rank::Jack, Suit::Spades),
        card(Rank::Three, Suit::Diamonds),
        card(Rank::Two, Suit::Hearts),
    ];
    let score_fl1 = evaluate_7cards(&hole_fl1, &board_fl);
    let score_fl2 = evaluate_7cards(&hole_fl2, &board_fl);
    assert_eq!(score_fl1 >> 32, 5);
    assert_eq!(score_fl2 >> 32, 5);
    assert!(score_fl1 > score_fl2, "Higher 5th flush card must win");

    /* 3. Full House: KKK22 vs QQQAA (Trips rank decides) */
    let hole_fh_k = [card(Rank::King, Suit::Spades), card(Rank::King, Suit::Hearts)];
    let board_fh_k = [
        card(Rank::King, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
        card(Rank::Two, Suit::Diamonds),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Eight, Suit::Clubs),
    ];
    let hole_fh_q = [card(Rank::Queen, Suit::Spades), card(Rank::Queen, Suit::Hearts)];
    let board_fh_q = [
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Ace, Suit::Clubs),
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Eight, Suit::Clubs),
    ];
    let score_fh_k = evaluate_7cards(&hole_fh_k, &board_fh_k);
    let score_fh_q = evaluate_7cards(&hole_fh_q, &board_fh_q);
    assert_eq!(score_fh_k >> 32, 6);
    assert_eq!(score_fh_q >> 32, 6);
    assert!(score_fh_k > score_fh_q, "KKK22 must beat QQQAA");

    /* 4. Three pairs on 7 cards (AA KK QQ): best 2 pairs chosen (AA-KK) with Q kicker */
    let hole_3p1 = [card(Rank::Ace, Suit::Spades), card(Rank::King, Suit::Spades)];
    let board_3p = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
        card(Rank::Two, Suit::Clubs),
    ];
    let score_3p1 = evaluate_7cards(&hole_3p1, &board_3p);
    assert_eq!(score_3p1 >> 32, 2, "Must be Two Pair category 2");
    assert_eq!((score_3p1 >> 16) & 0xFF, 12, "Top pair Ace (12)");
    assert_eq!((score_3p1 >> 8) & 0xFF, 11, "Second pair King (11)");
    assert_eq!(score_3p1 & 0xFF, 10, "Kicker Queen (10)");
}

#[test]
fn test_subgame_solver_adaptive_budget() {
    use lil_poker_mccfr::cfr::subgame::SubgameSolver;

    let solver = SubgameSolver::new(2000);
    assert!(solver.adaptive);

    /* 1. Small pot on flop (pot 40, round 2) -> scaled down for rapid response */
    let small_budget = solver.compute_budget(2, [20, 20], 0);
    assert!(
        small_budget < 2000,
        "Small pot should use less than base budget, got {}",
        small_budget
    );

    /* 2. Big pot on river (pot 800, round 4) -> scaled up for maximum precision */
    let big_river_budget = solver.compute_budget(4, [400, 400], 0);
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

    /* 3. Facing bet (to_call 150) -> precision boost */
    let facing_bet_budget = solver.compute_budget(3, [200, 350], 0);
    let passive_budget = solver.compute_budget(3, [200, 200], 0);
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