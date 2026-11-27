mod open_and_3bet;
mod shove_and_nash;

use crate::opponent::hole;
use lil_poker_mccfr::cfr::abstraction::preflop_bucket;
use lil_poker_mccfr::cfr::opponent_model::{OpponentTracker, ShortStackRequest};
use lil_poker_mccfr::game::config::{HOLDEM_NUM_ACTIONS, RAISE_MIN};

pub(super) fn pf_bucket(cards: &str) -> usize {
    let h = hole(cards);
    preflop_bucket(h[0], h[1]).0
}

pub(super) fn raise_count(counts: &[usize; HOLDEM_NUM_ACTIONS]) -> usize {
    counts[RAISE_MIN as usize..].iter().sum()
}

pub(super) fn assert_short_stack(
    tracker: &OpponentTracker,
    req: ShortStackRequest<'_>,
    expected: u8,
    msg: &str,
) {
    let (act, reason) = tracker
        .select_short_stack_preflop_action(&req)
        .unwrap_or_else(|| panic!("{msg}: short-stack engine inactive"));
    assert_eq!(act, expected, "{msg}, got {act} ({reason})");
}
