/*
 * Strategy Fallback Engine and In-Memory Lookup Index.
 */

pub mod heuristic;
pub mod index;

pub use heuristic::{
    get_holdem_fallback_strategy, lookup_strategy_hierarchical, lookup_strategy_hierarchical_owned,
};
pub use index::StrategyLookupIndex;
