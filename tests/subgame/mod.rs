mod reach;
mod solver;

use lil_poker_mccfr::cfr::subgame::OpponentReachDistribution;
use lil_poker_mccfr::game::holdem::{ALL_52_CARDS, Card};

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
