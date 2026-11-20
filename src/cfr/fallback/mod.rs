/*!
 * Strategy Fallback Engine and In-Memory Lookup Index.
 */

pub mod heuristic;
pub mod index;

pub use heuristic::{
    get_holdem_fallback_strategy, get_postflop_fallback_by_bucket, lookup_strategy_hierarchical,
    lookup_strategy_hierarchical_owned, lookup_strategy_hierarchical_owned_with_stack,
    lookup_strategy_hierarchical_with_stack, FallbackRequest,
};
pub use index::StrategyLookupIndex;
