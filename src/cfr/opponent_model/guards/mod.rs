/*! Priority-Ordered Tactical Guards Architecture:
 * - context / helpers: precomputed decision context and zero-cost probability helpers
 * - subgame: Essential safety invariants for Turn/River online subgame solves
 * - Tier 1 (`tier1_boosts`): Soft value/sizing boosts, button steals, and blocker bluffs
 * - Tier 2 (`tier2_preflop`, `tier2_shove` & `tier2_caps`): Hard preflop discipline, shove defense, raise caps, and pot control
 * - Tier 3 (`station::apply_postflop_barrel_and_pot_commitment_guards`): Terminal barrel folds & pot-commitment rescue
 */
mod context;
mod guard_id;
mod helpers;
mod post_selection;
mod shared_postflop;
mod subgame;
mod tier1_boosts;
mod tier2_caps;
mod tier2_preflop;
mod tier2_shove;

use super::tracker::OpponentTracker;
use crate::game::config::ROUND_TURN;
pub(super) use context::{GuardContext, GuardInput};
pub use guard_id::{GuardId, GuardSet, GuardSetIter};
pub(super) use helpers::{action_idx, first_action_idx, force_action, zero_and_sum};
pub(super) use shared_postflop::{
    is_turn_commit_underpair_fold, is_turn_paired_or_flush_underpair_fold,
};

impl OpponentTracker {
    pub(super) fn apply_preflop_and_postflop_guards(
        &self,
        probs: &mut [f64],
        input: GuardInput<'_>,
        reasons: &mut GuardSet,
    ) {
        let ctx = GuardContext::new(self, input);

        /* Priority Tier 1: Soft Value / Sizing Shifts & Situational Exploits */
        self.apply_tier1_soft_boosts(probs, &ctx, reasons);

        /* Turn/River SubgameSolver solutions keep essential safety invariants while preserving GTO equilibrium */
        if ctx.from_subgame && ctx.round >= ROUND_TURN {
            self.apply_subgame_postflop_guards(probs, &ctx, reasons);
            return;
        }

        /* Priority Tier 2A: Preflop Discipline & All-In / Heavy-Bet Shove Defense */
        self.apply_tier2_preflop_and_shove_defense(probs, &ctx, reasons);

        /* Priority Tier 2B: Postflop Hard Raise Caps, Draw Odds & Pot Control */
        self.apply_tier2_postflop_caps_and_pot_control(probs, &ctx, reasons);

        /* Multi-Way Table Adaptation (3 to 6 players active) — runs BEFORE Tier 3 so terminal
         * pot-commitment rescues cannot be overwritten by multi-way folds */
        if ctx.active_opponents >= 2 {
            reasons.insert(GuardId::MultiwayAdapt);
            Self::apply_multiway_adaptation(probs, &ctx);
        }

        /* Priority Tier 3: Terminal Barrel Defense & Unconditional Pot-Commitment */
        self.apply_postflop_barrel_and_pot_commitment_guards(probs, &ctx, reasons);
    }
}
