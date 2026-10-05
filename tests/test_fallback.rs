use lil_poker_mccfr::cfr::fallback::get_holdem_fallback_strategy;
use lil_poker_mccfr::game::holdem::{
    Card, Rank, Suit, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};

#[test]
fn test_fallback_strategy_preflop_and_postflop() {
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
