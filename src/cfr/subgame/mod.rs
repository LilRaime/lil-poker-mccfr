/*!
 * Real-Time Subgame Search (Subgame Solving) for Texas Hold'em.
 * Solves depth-limited subgame online at Turn/River nodes during live play.
 */

pub mod reach;
pub mod solver;
mod traversal;

pub use reach::{
    CachedStreetReach, NUM_HOLE_COMBOS, OpponentReachDistribution, ReachRequest, combo_index,
};
pub use solver::{OppStreetProfile, SubgameContext, SubgameRequest, SubgameSolver};
