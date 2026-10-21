use lil_poker_mccfr::cfr::abstraction::{
    get_holdem_infoset_key, get_holdem_infoset_key_rich, hash_holdem_infoset_key,
    hash_holdem_infoset_key_rich,
};
use lil_poker_mccfr::game::holdem::{Card, Rank, Suit};

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

    let hash = hash_holdem_infoset_key(&hole, &board, 2, &history_dummy);
    assert_ne!(hash, 0);
}

#[test]
fn test_rich_infoset_key_generation() {
    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let board = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];

    /* History: Preflop raise-call [2, 1], Flop check [1] */
    let history = [vec![2u8, 1u8], vec![1u8], vec![], vec![]];

    let base_key = get_holdem_infoset_key(&hole, &board, 2, &history);
    let rich_key = get_holdem_infoset_key_rich(&hole, &board, 2, &history);

    assert_eq!(base_key, "F:B39/c");
    assert_eq!(rich_key, "P:rc|F:B39/c");

    let base_hash = hash_holdem_infoset_key(&hole, &board, 2, &history);
    let rich_hash = hash_holdem_infoset_key_rich(&hole, &board, 2, &history);
    assert_ne!(base_hash, 0);
    assert_ne!(rich_hash, 0);
    assert_ne!(base_hash, rich_hash);
}

#[test]
fn test_board_texture_codes() {
    use lil_poker_mccfr::cfr::abstraction::board_texture_code;
    let card = |rank, suit| Card { rank, suit };

    /* Rainbow Unpaired: A-7-2 rainbow */
    let board_rainbow = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];
    assert_eq!(board_texture_code(&board_rainbow), "R");

    /* Two-Tone Unpaired: A-7-2 two-tone (Diamonds, Diamonds, Hearts) */
    let board_twotone = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Two, Suit::Hearts),
    ];
    assert_eq!(board_texture_code(&board_twotone), "T");

    /* Monotone: A-7-2 all Hearts */
    let board_monotone = [
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Two, Suit::Hearts),
    ];
    assert_eq!(board_texture_code(&board_monotone), "M");

    /* Paired: 7-7-2 rainbow */
    let board_paired = [
        card(Rank::Seven, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];
    assert_eq!(board_texture_code(&board_paired), "RP");
}

#[test]
fn test_multi_street_rich_history_generation() {
    use lil_poker_mccfr::cfr::abstraction::{
        get_holdem_infoset_key_rich, hash_holdem_infoset_key_rich,
    };
    use lil_poker_mccfr::game::holdem::{Card, Rank, Suit};
    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    let board = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
        card(Rank::Ten, Suit::Spades),
        card(Rank::King, Suit::Clubs),
    ];

    /* Turn history: Preflop raise-call [2, 1], Flop bet-call [3, 1], Turn check [1] */
    let turn_history = [vec![2u8, 1u8], vec![3u8, 1u8], vec![1u8], vec![]];
    let turn_key = get_holdem_infoset_key_rich(&hole, &board[..4], 3, &turn_history);
    assert_eq!(turn_key, "P:rc|F:rc|T:B39/c");

    let turn_hash = hash_holdem_infoset_key_rich(&hole, &board[..4], 3, &turn_history);
    assert_ne!(turn_hash, 0);

    /* River history: Preflop rc [2, 1], Flop rc [4, 1], Turn cc [1, 1], River bet [3] */
    let river_history = [vec![2u8, 1u8], vec![4u8, 1u8], vec![1u8, 1u8], vec![3u8]];
    let river_key = get_holdem_infoset_key_rich(&hole, &board, 4, &river_history);
    assert_eq!(river_key, "R:B52/t");
}

#[test]
fn test_two_pair_bottom_pair_on_board_abstraction() {
    use lil_poker_mccfr::cfr::abstraction::get_holdem_infoset_key;
    use lil_poker_mccfr::game::holdem::{Card, Rank, Suit};
    let card = |rank, suit| Card { rank, suit };
    let hole = [
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Hearts),
    ];
    /* Board has pair of Twos; player hits top pair Ace with King kicker */
    let board = [
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
        card(Rank::Ten, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];
    let river_history = [vec![2u8, 1u8], vec![4u8, 1u8], vec![1u8, 1u8], vec![3u8]];
    let river_key = get_holdem_infoset_key(&hole, &board, 4, &river_history);
    /* Should be bucket 50 (Top and Bottom Pair where bottom pair is on board), NOT bucket 52 */
    assert_eq!(river_key, "R:B50/t");
}

#[test]
fn test_v8_board_trips_paired_boards_and_draws() {
    use lil_poker_mccfr::cfr::abstraction::{detect_draws_detailed, postflop_equity_bucket};
    let card = |rank, suit| Card { rank, suit };

    /* 1. Hand 3 bug: 9h 5c on Jd Js Jc 3s 2c (Board Trips) must NOT be bucket 56! */
    let hole_95 = [
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Five, Suit::Clubs),
    ];
    let board_jjj = [
        card(Rank::Jack, Suit::Diamonds),
        card(Rank::Jack, Suit::Spades),
        card(Rank::Jack, Suit::Clubs),
        card(Rank::Three, Suit::Spades),
        card(Rank::Two, Suit::Clubs),
    ];
    let b_95 = postflop_equity_bucket(&hole_95, &board_jjj);
    assert!(
        b_95 <= 9,
        "95o on J-J-J-3-2 board trips must be pure air bucket <= 9, got {}",
        b_95
    );

    /* 2. Hand 2 bug: Jd Qc on Qh 8c 3c Kc (3-club board with 1-card Qc draw) must NOT be bucket 41 */
    let hole_jq = [
        card(Rank::Jack, Suit::Diamonds),
        card(Rank::Queen, Suit::Clubs),
    ];
    let board_3club = [
        card(Rank::Queen, Suit::Hearts),
        card(Rank::Eight, Suit::Clubs),
        card(Rank::Three, Suit::Clubs),
        card(Rank::King, Suit::Clubs),
    ];
    let draws_jq = detect_draws_detailed(&hole_jq, &board_3club);
    assert!(
        !draws_jq.is_flush_draw,
        "Weak 1-card Qc flush draw on 3-club board should not trigger immortal flush draw"
    );

    /* 3. Bug #18: 4-to-a-straight on board (5-6-7-8) where player holds duplicated 7 must NOT be OESD */
    let hole_72 = [
        card(Rank::Seven, Suit::Hearts),
        card(Rank::Two, Suit::Clubs),
    ];
    let board_5678 = [
        card(Rank::Five, Suit::Diamonds),
        card(Rank::Six, Suit::Spades),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Eight, Suit::Hearts),
    ];
    let draws_72 = detect_draws_detailed(&hole_72, &board_5678);
    assert!(
        !draws_72.is_straight_draw,
        "Duplicated board rank on 4-to-straight board must not be OESD"
    );

    /* 4. Bug #20: Q8o on A-9-3 flop must use hole card high rank (Queen -> 6), not board Ace (10) */
    let hole_q8 = [
        card(Rank::Queen, Suit::Spades),
        card(Rank::Eight, Suit::Diamonds),
    ];
    let board_a93 = [
        card(Rank::Ace, Suit::Clubs),
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Three, Suit::Spades),
    ];
    assert_eq!(postflop_equity_bucket(&hole_q8, &board_a93), 6);
}
