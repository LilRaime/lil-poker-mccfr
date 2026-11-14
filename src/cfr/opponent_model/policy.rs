/* Purified Action Sampling and Orchestration Pipeline */
use super::tracker::OpponentTracker;
use crate::game::config::*;
use crate::game::holdem::Card;
use rand::Rng;

/* Structured diagnostic trace of a purified action decision */
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecisionTrace {
    pub bucket: usize,
    pub round: u8,
    pub pot_odds_bps: u16,
    pub raw_top_action: u8,
    pub final_action: u8,
    pub guard_reasons: Vec<&'static str>,
}

impl OpponentTracker {
    /* Purified and noise-filtered action sampling with all-in defense (backward-compatible) */
    pub fn select_action_purified<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        is_preflop: bool,
        to_call: i32,
        rng: &mut R,
    ) -> u8 {
        self.select_action_purified_round(
            raw_probs,
            legal_actions,
            bucket,
            if is_preflop { 1 } else { 2 },
            to_call,
            rng,
        )
    }

    /* Street-aware purified action sampling */
    pub fn select_action_purified_round<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        rng: &mut R,
    ) -> u8 {
        self.select_action_purified_context(
            raw_probs,
            legal_actions,
            bucket,
            round,
            to_call,
            to_call.max(BIG_BLIND) * 2,
            None,
            None,
            rng,
        )
    }

    /* Full contextual purified action sampling with hole and board awareness for blocker effects and pot odds */
    #[allow(clippy::too_many_arguments)]
    pub fn select_action_purified_context<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        pot: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        rng: &mut R,
    ) -> u8 {
        self.select_action_purified_context_multiway(
            raw_probs,
            legal_actions,
            bucket,
            round,
            to_call,
            pot,
            hole,
            board,
            1,
            rng,
        )
    }

    /* Multi-way aware purified action sampling (adapts ranges, bluff frequencies, and pot-control for 2..=6 players) */
    #[allow(clippy::too_many_arguments)]
    pub fn select_action_purified_context_multiway<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        pot: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        active_opponents: usize,
        rng: &mut R,
    ) -> u8 {
        self.select_action_purified_context_full(
            raw_probs,
            legal_actions,
            bucket,
            round,
            to_call,
            pot,
            hole,
            board,
            active_opponents,
            BIG_BLIND,
            rng,
        )
    }

    /* Blind-level and Multi-way aware purified action sampling */
    #[allow(clippy::too_many_arguments)]
    pub fn select_action_purified_context_full<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        pot: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        active_opponents: usize,
        table_bb: i32,
        rng: &mut R,
    ) -> u8 {
        self.select_action_purified_traced(
            raw_probs,
            legal_actions,
            bucket,
            round,
            to_call,
            pot,
            hole,
            board,
            active_opponents,
            table_bb,
            rng,
        )
        .0
    }

    /* Full contextual purified action sampling that also returns a structured DecisionTrace */
    #[allow(clippy::too_many_arguments)]
    pub fn select_action_purified_traced<R: Rng>(
        &self,
        raw_probs: &[f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        pot: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        active_opponents: usize,
        table_bb: i32,
        rng: &mut R,
    ) -> (u8, DecisionTrace) {
        let bb = if table_bb > 0 { table_bb } else { BIG_BLIND };
        let eff_allin_threshold = (bb * 35).max(1);
        let active_opponents = active_opponents.max(1);
        let raw_top_action = legal_actions
            .iter()
            .copied()
            .max_by(|&a, &b| {
                let pa = raw_probs.get(a as usize).copied().unwrap_or(0.0);
                let pb = raw_probs.get(b as usize).copied().unwrap_or(0.0);
                pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(CALL_CHECK);
        let adjusted_probs =
            self.adjust_strategy_context(raw_probs, legal_actions, bucket, round, to_call, bb);

        let mut probs: Vec<f64> = legal_actions
            .iter()
            .map(|&a| {
                let idx = a as usize;
                if idx < adjusted_probs.len() {
                    adjusted_probs[idx].max(0.0)
                } else {
                    0.0
                }
            })
            .collect();

        let total_pot = pot + to_call;
        let pot_odds = if total_pot > 0 {
            to_call as f64 / total_pot as f64
        } else {
            1.0
        };
        let mut reasons: Vec<&'static str> = Vec::with_capacity(4);

        let is_three_flush = board
            .map(|b| {
                let mut sc = [0u8; 4];
                for c in b {
                    sc[c.suit as usize] += 1;
                }
                sc.iter().any(|&cnt| cnt >= 3)
            })
            .unwrap_or(false);

        /* Priority Pipeline (Tier 1 Soft Shifts -> Tier 2 Hard Raise Caps -> Tier 3 Terminal Fold / Pot-Commit) */
        self.apply_preflop_and_postflop_guards(
            &mut probs,
            legal_actions,
            bucket,
            round,
            to_call,
            pot,
            hole,
            board,
            active_opponents,
            bb,
            eff_allin_threshold,
            pot_odds,
            is_three_flush,
            &mut reasons,
        );

        /* Multi-Way Table Adaptation (3 to 6 players active) */
        if active_opponents >= 2 {
            reasons.push("MULTIWAY_ADAPT");
        }
        self.apply_multiway_adaptation(
            &mut probs,
            legal_actions,
            bucket,
            round,
            to_call,
            board,
            active_opponents,
            bb,
            pot_odds,
            is_three_flush,
        );

        /* 5. Noise Clamping:
         * Suppress tiny exploration noise (< 2.5%) if a dominant legal move exists.
         */
        let max_prob = probs.iter().copied().fold(0.0f64, f64::max);
        if max_prob > 0.20 {
            for p in probs.iter_mut() {
                if *p < 0.025 {
                    *p = 0.0;
                }
            }
        }

        /* 6. Mild Temperature Sharpening (T = 0.85 => power 1.18):
         * Gently cleans up low-probability actions without distorting GTO balance.
         */
        let temp_inv = 1.18;
        for p in probs.iter_mut() {
            if *p > 0.0 {
                *p = p.powf(temp_inv);
            }
        }

        /* 7. Normalization and Sampling */
        let sum: f64 = probs.iter().sum();
        let chosen = if sum > 1e-12 {
            let norm: Vec<f64> = probs.iter().map(|p| p / sum).collect();
            let r: f64 = rng.gen();
            let mut cum = 0.0;
            let mut picked = *legal_actions.last().unwrap();
            for (i, &p) in norm.iter().enumerate() {
                cum += p;
                if r < cum {
                    picked = legal_actions[i];
                    break;
                }
            }
            picked
        } else if let Some(&c) = legal_actions.iter().find(|&&a| a == CALL_CHECK) {
            c
        } else {
            *legal_actions.first().unwrap_or(&1)
        };

        let trace = DecisionTrace {
            bucket,
            round,
            pot_odds_bps: (pot_odds.clamp(0.0, 1.0) * 1000.0).round() as u16,
            raw_top_action,
            final_action: chosen,
            guard_reasons: reasons,
        };
        (chosen, trace)
    }
}
