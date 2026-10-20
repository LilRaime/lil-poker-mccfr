/*
 * Real-Time Subgame Search (Subgame Solving) for Texas Hold'em.
 * Solves depth-limited subgame online at Turn/River nodes during live play.
 */

pub mod reach;
pub mod solver;

pub use reach::OpponentReachDistribution;
pub use solver::{SubgameContext, SubgameSolver};
