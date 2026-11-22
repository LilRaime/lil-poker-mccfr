use super::{combo_weight, remaining_deck_excluding};
use lil_poker_mccfr::cfr::subgame::{OpponentReachDistribution, ReachRequest};
use lil_poker_mccfr::game::holdem::{
    CALL_CHECK, Card, NUM_ROUNDS, RAISE_HALF_POT, Rank, RoundHistory, Suit,
};
use rand::SeedableRng;
use std::collections::HashMap;

#[test]
fn test_opponent_reach_distribution() {
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

    let used = [hole[0], hole[1], board[0], board[1], board[2]];
    let remaining = remaining_deck_excluding(&used);

    /* Scenario 1: Opponent raised preflop (strong range) */
    let history_raised = [vec![1u8, 2u8], vec![], vec![], vec![]];
    let dist_raised = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining,
            board: &board,
            round: 2,
            ..Default::default()
        },
        &history_raised,
    );
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
    let dist_limped = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining,
            board: &board,
            round: 2,
            ..Default::default()
        },
        &history_limped,
    );
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

#[test]
fn test_opponent_reach_with_strategy_map() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::Two, Suit::Spades),
        card(Rank::Three, Suit::Hearts),
    ];
    let board = [
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Eight, Suit::Clubs),
        card(Rank::Nine, Suit::Hearts),
    ];
    let used = [hole[0], hole[1], board[0], board[1], board[2]];
    let remaining = remaining_deck_excluding(&used);

    let mut strat_map = HashMap::new();
    strat_map.insert("P:AA/".to_string(), vec![0.0, 0.0, 0.5, 0.3, 0.2, 0.0]);
    strat_map.insert("P:72o/".to_string(), vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);

    let history_raised = [vec![1u8, 2u8], vec![], vec![], vec![]];
    let dist = OpponentReachDistribution::build_with_strategy(
        &ReachRequest {
            remaining_deck: &remaining,
            board: &board,
            round: 2,
            ..Default::default()
        },
        &history_raised,
        Some(&strat_map),
    );
    assert!(dist.total_weight > 0.0);
    assert_eq!(
        dist.combos.len(),
        remaining.len() * (remaining.len() - 1) / 2
    );
}

#[test]
fn test_opponent_reach_distribution_position_aware() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::King, Suit::Clubs),
        card(Rank::Queen, Suit::Diamonds),
    ];
    let board = [
        card(Rank::Jack, Suit::Hearts),
        card(Rank::Ten, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];

    let used = [hole[0], hole[1], board[0], board[1], board[2]];
    let remaining = remaining_deck_excluding(&used);

    /* History: Action 0 (Player 0) = Raise (2), Action 1 (Player 1) = Call (1) */
    let history = [vec![2u8, 1u8], vec![], vec![], vec![]];

    /* If my_player == 0: Bot raised, opponent called. Opponent range should be CALLER range */
    let dist_bot_raised = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining,
            board: &board,
            round: 2,
            target_player: Some(1),
            active_player: Some(0),
            ..Default::default()
        },
        &history,
    );

    /* If my_player == 1: Opponent raised, bot called. Opponent range should be RAISER range */
    let dist_opp_raised = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining,
            board: &board,
            round: 2,
            target_player: Some(0),
            active_player: Some(1),
            ..Default::default()
        },
        &history,
    );

    /* Find weight for a premium hand like AA */
    let w_aa_caller = combo_weight(
        &dist_bot_raised,
        card(Rank::Ace, Suit::Spades),
        card(Rank::Ace, Suit::Hearts),
    );
    let w_aa_raiser = combo_weight(
        &dist_opp_raised,
        card(Rank::Ace, Suit::Spades),
        card(Rank::Ace, Suit::Hearts),
    );

    /* Raiser has full weight on AA (1.0), whereas caller has discounted slowplay weight (0.35) */
    assert!(
        w_aa_raiser > w_aa_caller,
        "AA weight for raiser ({w_aa_raiser}) must exceed AA weight for caller ({w_aa_caller})"
    );
    assert_eq!(w_aa_raiser, 1.0);
    assert_eq!(w_aa_caller, 0.35);

    /* AKo is Tier 1, so for preflop raiser it must have weight 1.0, while 72o junk drops to 0.04 */
    let w_ako = combo_weight(
        &dist_opp_raised,
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    );
    let w_72 = combo_weight(
        &dist_opp_raised,
        card(Rank::Seven, Suit::Spades),
        card(Rank::Two, Suit::Hearts),
    );
    assert_eq!(
        w_ako, 1.0,
        "AKo for preflop raiser must have full 1.0 weight, got {w_ako}"
    );
    assert_eq!(
        w_72, 0.04,
        "72o for preflop raiser must have trash 0.04 weight, got {w_72}"
    );
}

#[test]
fn test_opponent_reach_distribution_3bet_and_limp_lines() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [card(Rank::Two, Suit::Clubs), card(Rank::Three, Suit::Clubs)];
    let board = [
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Eight, Suit::Hearts),
        card(Rank::Nine, Suit::Spades),
    ];

    let used = [hole[0], hole[1], board[0], board[1], board[2]];
    let remaining = remaining_deck_excluding(&used);

    /* 1. Opponent 3-Bet Line: Bot raises (action 0 = 2), Opponent 3-bets (action 1 = 4) */
    let history_3bet = [vec![2u8, 4u8], vec![], vec![], vec![]];
    let dist_3bet = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining,
            board: &board,
            round: 2,
            target_player: Some(1), /* My player is 0, so opponent is 1 (the 3-bettor) */
            active_player: Some(0),
            ..Default::default()
        },
        &history_3bet,
    );

    let aa_s = card(Rank::Ace, Suit::Spades);
    let aa_h = card(Rank::Ace, Suit::Hearts);
    let junk_7 = card(Rank::Seven, Suit::Clubs);
    let junk_2 = card(Rank::Two, Suit::Diamonds);

    assert_eq!(
        combo_weight(&dist_3bet, aa_s, aa_h),
        1.0,
        "3-bet range must include AA at 1.0 weight"
    );
    assert_eq!(
        combo_weight(&dist_3bet, junk_7, junk_2),
        0.01,
        "3-bet range must filter out 72o junk to 0.01"
    );

    /* 2. Limped Pot Line: Both players limp/check (actions = [1, 1]) */
    let history_limp = [vec![1u8, 1u8], vec![], vec![], vec![]];
    let dist_limp = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining,
            board: &board,
            round: 2,
            target_player: Some(1),
            active_player: Some(0),
            ..Default::default()
        },
        &history_limp,
    );

    assert_eq!(
        combo_weight(&dist_limp, aa_s, aa_h),
        0.08,
        "Limped pot caps AA to slowplay frequency 0.08"
    );
    assert_eq!(
        combo_weight(
            &dist_limp,
            card(Rank::Eight, Suit::Spades),
            card(Rank::Eight, Suit::Clubs)
        ),
        1.00,
        "Limped pot contains mid pairs at 1.00 weight"
    );
}

#[test]
fn test_opponent_reach_distribution_monotone_texture() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [card(Rank::Two, Suit::Clubs), card(Rank::Three, Suit::Clubs)];
    /* Monotone board with 3 Spades */
    let board = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::Seven, Suit::Spades),
        card(Rank::Four, Suit::Spades),
    ];

    let used = [hole[0], hole[1], board[0], board[1], board[2]];
    let remaining = remaining_deck_excluding(&used);

    /* History: Bot calls (1), Opponent raises (2) on flop */
    let history = [vec![2u8, 1u8], vec![2u8], vec![], vec![]];
    let dist = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining,
            board: &board,
            round: 2,
            target_player: Some(0), /* My player is 1, so opponent is 0 (the raiser on flop) */
            active_player: Some(1),
            ..Default::default()
        },
        &history,
    );

    /* Candidate 1: Made flush with 2 spades (K♠ Q♠) vs Candidate 2: Offsuit no-flush cards (K♥ Q♦) */
    let w_flush = combo_weight(
        &dist,
        card(Rank::King, Suit::Spades),
        card(Rank::Queen, Suit::Spades),
    );
    let w_no_spade = combo_weight(
        &dist,
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
    );

    assert!(
        w_flush > w_no_spade * 1.5,
        "Opponent raising on monotone board should weight flush combo ({w_flush}) much higher than no-spade combo ({w_no_spade})"
    );
}

#[test]
fn test_vpip_adapted_bayesian_reach_narrowing() {
    let board_flop = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let hole_bot = [
        Card::new(Rank::Queen, Suit::Diamonds),
        Card::new(Rank::Jack, Suit::Diamonds),
    ];
    let used = [
        board_flop[0],
        board_flop[1],
        board_flop[2],
        hole_bot[0],
        hole_bot[1],
    ];
    let remaining_deck = remaining_deck_excluding(&used);
    let history = [vec![RAISE_HALF_POT, CALL_CHECK], vec![], vec![], vec![]];

    let dist_nit = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining_deck,
            board: &board_flop,
            round: 2,
            target_player: Some(1),
            active_player: Some(0),
            opp_vpip: Some(0.15),
            opp_pfr: Some(0.12),
        },
        &history,
    );
    let dist_loose = OpponentReachDistribution::build(
        &ReachRequest {
            remaining_deck: &remaining_deck,
            board: &board_flop,
            round: 2,
            target_player: Some(1),
            active_player: Some(0),
            opp_vpip: Some(0.75),
            opp_pfr: Some(0.10),
        },
        &history,
    );

    let c7h = Card::new(Rank::Seven, Suit::Hearts);
    let c2d = Card::new(Rank::Two, Suit::Diamonds);
    let p_nit = combo_weight(&dist_nit, c7h, c2d) / dist_nit.total_weight.max(1e-12);
    let p_loose = combo_weight(&dist_loose, c7h, c2d) / dist_loose.total_weight.max(1e-12);
    assert!(
        p_loose > p_nit,
        "Loose VPIP opponent must assign higher relative reach probability to weak offsuit combos ({p_loose:.6} vs {p_nit:.6})"
    );
}

#[test]
fn test_subgame_reach_active_street_actor_separation() {
    let board = [
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Diamonds),
        Card::new(Rank::Queen, Suit::Clubs),
    ];
    let used = [
        board[0],
        board[1],
        board[2],
        Card::new(Rank::Jack, Suit::Hearts),
        Card::new(Rank::Ten, Suit::Clubs),
    ];
    let rem = remaining_deck_excluding(&used);
    let mut history = [RoundHistory::new(); NUM_ROUNDS];
    history[0].push(CALL_CHECK);
    history[0].push(CALL_CHECK);
    history[1].push(CALL_CHECK);
    history[1].push(RAISE_HALF_POT);

    let no_map: Option<&HashMap<String, Vec<f64>>> = None;

    /* With active_player == Some(0) and F=[CALL_CHECK, RAISE_HALF_POT], target=0's action is idx 0 (check).
     * With active_player == Some(1) (inverted/opponent turn), target=0 is attributed idx 1 (half-pot bet). */
    let target0_as_checker = OpponentReachDistribution::build_with_strategy(
        &ReachRequest {
            remaining_deck: &rem,
            board: &board,
            round: 2,
            target_player: Some(0),
            active_player: Some(0),
            ..Default::default()
        },
        &history,
        no_map,
    );
    let target0_as_bettor = OpponentReachDistribution::build_with_strategy(
        &ReachRequest {
            remaining_deck: &rem,
            board: &board,
            round: 2,
            target_player: Some(0),
            active_player: Some(1),
            ..Default::default()
        },
        &history,
        no_map,
    );
    let air_weight = |dist: &OpponentReachDistribution| -> f64 {
        dist.combos
            .iter()
            .zip(dist.weights.iter())
            .find(|(c, _)| {
                (c[0].rank == Rank::Eight && c[1].rank == Rank::Three)
                    || (c[0].rank == Rank::Three && c[1].rank == Rank::Eight)
            })
            .map_or(0.0, |(_, &w)| w)
    };
    let w_check = air_weight(&target0_as_checker);
    let w_bet = air_weight(&target0_as_bettor);
    assert!(
        w_check > w_bet * 2.0 && target0_as_checker.total_weight > target0_as_bettor.total_weight,
        "Active-street parity must assign idx 0 (check, air w={w_check:.4}) to target=0 when active=0 rather than idx 1 (bet, air w={w_bet:.4})"
    );
}
