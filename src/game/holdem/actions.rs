/* Action constants and sizing for 52-Card Texas Hold'em. */
pub use crate::game::config::{
    ALL_IN, BIG_BLIND_F64, CALL_CHECK, DEFAULT_SUBGAME_ITERATIONS, FOLD, HOLDEM_NUM_ACTIONS,
    MAX_ROUND_ACTIONS, NUM_CARDS_DECK, NUM_COMMUNITY_CARDS, NUM_EQUITY_BUCKETS, NUM_HOLE_CARDS,
    NUM_PLAYERS, NUM_PREFLOP_BUCKETS, NUM_RANKS, NUM_ROUNDS, NUM_SUITS, RAISE_HALF_POT, RAISE_MIN,
    RAISE_THIRD_POT,
};

/* Calculates geometric bet sizing to smoothly commit remaining stack over remaining streets.
 * SPR = remaining_stack / pot.
 * Multiplier per street = ((1 + 2 * SPR)^(1 / streets_left) - 1) / 2. */
#[inline]
pub fn calculate_geometric_bet_size(pot: i32, remaining_stack: i32, streets_left: u32) -> i32 {
    use crate::game::config::BIG_BLIND;

    if pot <= 0 || remaining_stack <= 0 || streets_left == 0 {
        return (pot / 2).max(BIG_BLIND * 2);
    }
    if streets_left == 1 {
        /* Last street: commit remaining stack if reasonable, else pot-sized */
        return remaining_stack.min(pot);
    }
    let spr = remaining_stack as f64 / pot as f64;
    let factor = (1.0 + 2.0 * spr).powf(1.0 / streets_left as f64);
    let bet_fraction = ((factor - 1.0) / 2.0).clamp(0.20, 2.0);
    ((pot as f64 * bet_fraction).round() as i32)
        .min(remaining_stack)
        .max(BIG_BLIND * 2)
}
