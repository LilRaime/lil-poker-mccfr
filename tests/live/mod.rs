mod decision;
mod protocol;
mod state;

use lil_poker_mccfr::game::holdem::Card;
use lil_poker_mccfr::live::parse_card_str;

pub(crate) fn card(s: &str) -> Card {
    parse_card_str(s).unwrap_or_else(|| panic!("invalid card token: {s}"))
}

pub(crate) fn hole(s: &str) -> [Card; 2] {
    let list: Vec<Card> = s.split_whitespace().map(card).collect();
    [list[0], list[1]]
}

pub(crate) fn board(s: &str) -> Vec<Card> {
    if s.trim().is_empty() {
        Vec::new()
    } else {
        s.split_whitespace().map(card).collect()
    }
}
