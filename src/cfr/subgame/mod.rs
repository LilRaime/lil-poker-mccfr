/*!
 * Real-Time Subgame Search (Subgame Solving) for Texas Hold'em.
 * Solves depth-limited subgame online at Turn/River nodes during live play.
 */

pub mod reach;
pub mod solver;
mod traversal;

pub use reach::{OpponentReachDistribution, ReachRequest};
pub use solver::{SubgameContext, SubgameRequest, SubgameSolver};
