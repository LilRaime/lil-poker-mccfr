use lil_poker_mccfr::game::holdem::{Rank, Suit};
use lil_poker_mccfr::live::protocol::{map_action_index, parse_card_str, parse_json_i32};
use serde_json::json;

#[test]
fn test_live_parse_card_str() {
    let ace_spades = parse_card_str("As").expect("parse As");
    assert_eq!(ace_spades.rank, Rank::Ace);
    assert_eq!(ace_spades.suit, Suit::Spades);

    let ten_hearts = parse_card_str("10h").expect("parse 10h");
    assert_eq!(ten_hearts.rank, Rank::Ten);
    assert_eq!(ten_hearts.suit, Suit::Hearts);

    let ten_diamonds = parse_card_str("Td").expect("parse Td");
    assert_eq!(ten_diamonds.rank, Rank::Ten);
    assert_eq!(ten_diamonds.suit, Suit::Diamonds);

    let king_spades_unicode = parse_card_str("K♠").expect("parse unicode K♠");
    assert_eq!(king_spades_unicode.rank, Rank::King);
    assert_eq!(king_spades_unicode.suit, Suit::Spades);

    let quoted_queen_clubs = parse_card_str("'Q♣'").expect("parse quoted 'Q♣'");
    assert_eq!(quoted_queen_clubs.rank, Rank::Queen);
    assert_eq!(quoted_queen_clubs.suit, Suit::Clubs);

    assert!(parse_card_str("").is_none());
    assert!(parse_card_str("Xx").is_none());
}

#[test]
fn test_live_parse_json_i32() {
    let i = json!(42);
    assert_eq!(parse_json_i32(Some(&i)), 42);

    let f = json!(123.0);
    assert_eq!(parse_json_i32(Some(&f)), 123);

    let s = json!("500");
    assert_eq!(parse_json_i32(Some(&s)), 500);

    assert_eq!(parse_json_i32(None), 0);
}

#[test]
fn test_live_map_action_index() {
    let legal = vec![
        "fold".to_string(),
        "check".to_string(),
        "call".to_string(),
        "raise".to_string(),
        "allin".to_string(),
    ];

    /* Index 0 (Fold): Facing no bet (to_call == 0) -> should prefer check! */
    let (act_free, amt_free) =
        map_action_index(0, &legal, 0, 100, false).expect("map index 0 when free");
    assert_eq!(act_free, "check");
    assert_eq!(amt_free, 0);

    /* Index 0 (Fold): Facing bet (to_call > 0) -> should fold */
    let (act_facing_bet, _) =
        map_action_index(0, &legal, 50, 100, false).expect("map index 0 facing bet");
    assert_eq!(act_facing_bet, "fold");

    /* Index 1 (Call / Check): When to_call > 0 -> call */
    let (act_call, amt_call) =
        map_action_index(1, &legal, 50, 100, false).expect("map index 1 facing bet");
    assert_eq!(act_call, "call");
    assert_eq!(amt_call, 0);

    /* Index 4 (Raise Half Pot): dry vs wet board */
    let (_, amt_dry) = map_action_index(4, &legal, 0, 1000, false).expect("raise half dry");
    let (_, amt_wet) = map_action_index(4, &legal, 0, 1000, true).expect("raise half wet");
    assert!(
        amt_wet > amt_dry,
        "Wet board raise half-pot should size larger than dry board"
    );

    /* Index 5 (All-in) */
    let (act_allin, _) = map_action_index(5, &legal, 50, 100, false).expect("allin");
    assert_eq!(act_allin, "allin");
}
