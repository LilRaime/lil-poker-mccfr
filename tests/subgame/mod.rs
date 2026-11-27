mod reach;
mod solver;

use lil_poker_mccfr::cfr::subgame::OpponentReachDistribution;
use lil_poker_mccfr::game::holdem::{ALL_52_CARDS, Card};
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

pub(crate) fn remaining_deck_excluding(used: &[Card]) -> Vec<Card> {
    ALL_52_CARDS
        .iter()
        .copied()
        .filter(|c| !used.contains(c))
        .collect()
}

pub(crate) fn combo_weight(dist: &OpponentReachDistribution, c1: Card, c2: Card) -> f64 {
    dist.combos
        .iter()
        .zip(dist.weights.iter())
        .find(|(c, _)| (c[0] == c1 && c[1] == c2) || (c[0] == c2 && c[1] == c1))
        .map_or(0.0, |(_, &w)| w)
}
