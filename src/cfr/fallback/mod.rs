/*
 * Strategy Fallback Engine and In-Memory Lookup Index.
 */

pub mod heuristic;
pub mod index;

pub use heuristic::{
    get_holdem_fallback_strategy, get_holdem_fallback_strategy_bb, get_postflop_fallback_by_bucket,
    lookup_strategy_hierarchical, lookup_strategy_hierarchical_owned,
};
pub use index::StrategyLookupIndex;
