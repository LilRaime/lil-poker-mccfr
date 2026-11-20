use lil_poker_mccfr::cfr::subgame::{OpponentReachDistribution, ReachRequest};
use lil_poker_mccfr::game::holdem::{
    Card, Rank, RoundHistory, Suit, ALL_52_CARDS, CALL_CHECK, NUM_ROUNDS, RAISE_HALF_POT,
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

    let is_used = |c: Card| hole.contains(&c) || board.contains(&c);
    let remaining: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|&c| !is_used(c))
        .collect();

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
    let remaining: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|&c| !hole.contains(&c) && !board.contains(&c))
        .collect();

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

    let remaining: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|&c| !hole.contains(&c) && !board.contains(&c))
        .collect();

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
    let aa_pair = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts)];
    let idx_aa = dist_bot_raised
        .combos
        .iter()
        .position(|&c| {
            (c[0] == aa_pair[0] && c[1] == aa_pair[1]) || (c[0] == aa_pair[1] && c[1] == aa_pair[0])
        })
        .expect("AA combo must exist in remaining deck");

    let w_aa_caller = dist_bot_raised.weights[idx_aa];
    let w_aa_raiser = dist_opp_raised.weights[idx_aa];

    /* Raiser has full weight on AA (1.0), whereas caller has discounted slowplay weight (0.35) */
    assert!(
        w_aa_raiser > w_aa_caller,
        "AA weight for raiser ({}) must exceed AA weight for caller ({})",
        w_aa_raiser,
        w_aa_caller
    );
    assert_eq!(w_aa_raiser, 1.0);
    assert_eq!(w_aa_caller, 0.35);
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

    let remaining: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|&c| !hole.contains(&c) && !board.contains(&c))
        .collect();

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

    let aa_pair = [card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts)];
    let junk_72 = [
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Diamonds),
    ];

    let idx_aa = dist_3bet
        .combos
        .iter()
        .position(|&c| {
            (c[0] == aa_pair[0] && c[1] == aa_pair[1]) || (c[0] == aa_pair[1] && c[1] == aa_pair[0])
        })
        .expect("AA combo exists");
    let idx_72 = dist_3bet
        .combos
        .iter()
        .position(|&c| {
            (c[0] == junk_72[0] && c[1] == junk_72[1]) || (c[0] == junk_72[1] && c[1] == junk_72[0])
        })
        .expect("72 combo exists");

    assert_eq!(
        dist_3bet.weights[idx_aa], 1.0,
        "3-bet range must include AA at 1.0 weight"
    );
    assert_eq!(
        dist_3bet.weights[idx_72], 0.01,
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

    let mid_pair_88 = [
        card(Rank::Eight, Suit::Spades),
        card(Rank::Eight, Suit::Clubs),
    ];
    let idx_88 = dist_limp
        .combos
        .iter()
        .position(|&c| {
            (c[0] == mid_pair_88[0] && c[1] == mid_pair_88[1])
                || (c[0] == mid_pair_88[1] && c[1] == mid_pair_88[0])
        })
        .expect("88 combo exists");

    assert_eq!(
        dist_limp.weights[idx_aa], 0.08,
        "Limped pot caps AA to slowplay frequency 0.08"
    );
    assert_eq!(
        dist_limp.weights[idx_88], 1.00,
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

    let remaining: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|&c| !hole.contains(&c) && !board.contains(&c))
        .collect();

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

    /* Candidate 1: Made flush with 2 spades (K♠ Q♠) */
    let flush_combo = [
        card(Rank::King, Suit::Spades),
        card(Rank::Queen, Suit::Spades),
    ];
    /* Candidate 2: Offsuit no-flush cards (K♥ Q♦) */
    let no_spade_combo = [
        card(Rank::King, Suit::Hearts),
        card(Rank::Queen, Suit::Diamonds),
    ];

    let idx_flush = dist
        .combos
        .iter()
        .position(|&c| {
            (c[0] == flush_combo[0] && c[1] == flush_combo[1])
                || (c[0] == flush_combo[1] && c[1] == flush_combo[0])
        })
        .expect("K♠ Q♠ exists");
    let idx_no_spade = dist
        .combos
        .iter()
        .position(|&c| {
            (c[0] == no_spade_combo[0] && c[1] == no_spade_combo[1])
                || (c[0] == no_spade_combo[1] && c[1] == no_spade_combo[0])
        })
        .expect("K♥ Q♦ exists");

    assert!(
        dist.weights[idx_flush] > dist.weights[idx_no_spade] * 1.5,
        "Opponent raising on monotone board should weight flush combo ({}) much higher than no-spade combo ({})",
        dist.weights[idx_flush],
        dist.weights[idx_no_spade]
    );
}

#[test]
fn test_opponent_reach_distribution_raiser_ako_weight() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::Two, Suit::Diamonds),
        card(Rank::Three, Suit::Clubs),
    ];
    let board = [
        card(Rank::Jack, Suit::Hearts),
        card(Rank::Ten, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];

    let remaining: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|&c| !hole.contains(&c) && !board.contains(&c))
        .collect();

    /* Preflop action: Opponent (Player 0) raises (2), Bot (Player 1) calls (1) */
    let history = [vec![2u8, 1u8], vec![], vec![], vec![]];

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

    /* AKo combo: A♠ K♥ */
    let ako_combo = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let idx_ako = dist_opp_raised
        .combos
        .iter()
        .position(|&c| {
            (c[0] == ako_combo[0] && c[1] == ako_combo[1])
                || (c[0] == ako_combo[1] && c[1] == ako_combo[0])
        })
        .expect("AKo combo must exist in remaining deck");

    /* 72o junk combo: 7♠ 2♥ (2c is on board, so 7s 2h is valid) */
    let junk_72 = [
        card(Rank::Seven, Suit::Spades),
        card(Rank::Two, Suit::Hearts),
    ];
    let idx_72 = dist_opp_raised
        .combos
        .iter()
        .position(|&c| {
            (c[0] == junk_72[0] && c[1] == junk_72[1]) || (c[0] == junk_72[1] && c[1] == junk_72[0])
        })
        .expect("72o combo must exist in remaining deck");

    let w_ako = dist_opp_raised.weights[idx_ako];
    let w_72 = dist_opp_raised.weights[idx_72];

    /* AKo is Tier 1, so for raiser it must have weight 1.0 (not 0.25 as in buggy index matching!) */
    assert_eq!(
        w_ako, 1.0,
        "AKo for preflop raiser must have full 1.0 weight, got {}",
        w_ako
    );
    assert_eq!(
        w_72, 0.04,
        "72o for preflop raiser must have trash 0.04 weight, got {}",
        w_72
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
    let rem: Vec<Card> = ALL_52_CARDS
        .iter()
        .copied()
        .filter(|c| !used.contains(c))
        .collect();
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
            .map(|(_, &w)| w)
            .unwrap_or(0.0)
    };
    let w_check = air_weight(&target0_as_checker);
    let w_bet = air_weight(&target0_as_bettor);
    assert!(
        w_check > w_bet * 2.0 && target0_as_checker.total_weight > target0_as_bettor.total_weight,
        "Active-street parity must assign idx 0 (check, air w={:.4}) to target=0 when active=0 rather than idx 1 (bet, air w={:.4})",
        w_check,
        w_bet
    );
}
