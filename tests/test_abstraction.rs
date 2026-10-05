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
