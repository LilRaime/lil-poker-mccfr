/*
 * Explicit Bayesian reach distribution for opponent's private hole cards.
 */

mod postflop;
mod preflop;

use crate::game::config::*;
use crate::game::holdem::{Card, Rank, Suit};
use std::collections::HashMap;

use postflop::{flop_likelihood, monotone_texture_multiplier, river_likelihood, turn_likelihood};
use preflop::{preflop_prior_weight, style_width_multiplier};

#[derive(Debug, Clone)]
pub struct OpponentReachDistribution {
    pub combos: Vec<[Card; 2]>,
    pub weights: Vec<f64>,
    pub cdf: Vec<f64>,
    pub total_weight: f64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ReachContext {
    pub opp_raised_pf: bool,
    pub opp_called_pf: bool,
    pub opp_3bet_pf: bool,
    pub opp_limped_pf: bool,
    pub max_opp_raise_flop: u8,
    pub opp_raised_flop: bool,
    pub opp_called_flop: bool,
    pub max_opp_raise_turn: u8,
    pub opp_raised_turn: bool,
    pub opp_called_turn: bool,
    pub max_opp_raise_river: u8,
    pub opp_raised_river: bool,
    pub flop_board_paired: bool,
    pub turn_board_paired: bool,
    pub river_board_paired: bool,
    pub monotone_suit: Option<Suit>,
    pub has_flop_actions: bool,
    pub has_turn_actions: bool,
}

fn collect_target_actions(
    actions: &[u8],
    street: u8,
    round: u8,
    target_player: Option<usize>,
    active_player: Option<usize>,
) -> Vec<u8> {
    let Some(tp) = target_player else {
        return actions.to_vec();
    };
    actions
        .iter()
        .enumerate()
        .filter(|&(idx, _)| {
            if street == round {
                match active_player {
                    Some(ap) if tp == ap => (actions.len() - idx).is_multiple_of(2),
                    _ => (actions.len() - 1 - idx).is_multiple_of(2),
                }
            } else {
                (1 - idx % 2) == tp || actions.len() == 1
            }
        })
        .map(|(_, &a)| a)
        .collect()
}

fn is_prefix_paired(board: &[Card], len: usize) -> bool {
    if board.len() < len {
        return false;
    }
    let mut rc = [0u8; 13];
    for c in &board[..len] {
        rc[c.rank as usize] += 1;
    }
    rc.iter().any(|&cnt| cnt >= 2)
}

fn find_monotone_suit(board: &[Card]) -> Option<Suit> {
    let mut suit_counts = [0u8; 4];
    for c in board {
        suit_counts[c.suit as usize] += 1;
    }
    suit_counts
        .iter()
        .enumerate()
        .find(|(_, &cnt)| cnt >= 3)
        .map(|(s_idx, _)| match s_idx {
            0 => Suit::Clubs,
            1 => Suit::Diamonds,
            2 => Suit::Hearts,
            _ => Suit::Spades,
        })
}

fn extract_reach_context<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
    board: &[Card],
    round: u8,
    history: &H,
    target_player: Option<usize>,
    active_player: Option<usize>,
) -> ReachContext {
    let (opp_raised_pf, opp_called_pf, opp_3bet_pf, opp_limped_pf) = if let Some(tp) = target_player
    {
        let actions = history.actions_in_round(0);
        let opp_actions: Vec<u8> = actions
            .iter()
            .enumerate()
            .filter(|(idx, _)| idx % 2 == tp)
            .map(|(_, &a)| a)
            .collect();
        let opp_raised = opp_actions.iter().any(|&a| a >= RAISE_MIN);
        let opp_called = opp_actions.contains(&CALL_CHECK);

        let mut is_3bet = false;
        for (idx, &a) in actions.iter().enumerate() {
            if idx % 2 == tp && a >= RAISE_MIN {
                let prior_raises = actions[..idx].iter().filter(|&&pa| pa >= RAISE_MIN).count();
                if prior_raises >= 1 {
                    is_3bet = true;
                    break;
                }
            }
        }
        if opp_actions.iter().filter(|&&a| a >= RAISE_MIN).count() >= 2 {
            is_3bet = true;
        }
        let total_pf_raises = actions.iter().filter(|&&a| a >= RAISE_MIN).count();
        let is_limped =
            total_pf_raises == 0 && (opp_called || actions.iter().all(|&a| a == CALL_CHECK));
        (opp_raised, opp_called, is_3bet, is_limped)
    } else {
        let actions = history.actions_in_round(0);
        let total_pf_raises = actions.iter().filter(|&&a| a >= RAISE_MIN).count();
        let opp_raised = actions.iter().any(|&a| a >= RAISE_MIN);
        let opp_called = actions.contains(&CALL_CHECK);
        (
            opp_raised,
            opp_called,
            total_pf_raises >= 2,
            total_pf_raises == 0 && actions.iter().all(|&a| a == CALL_CHECK),
        )
    };

    let flop_actions = collect_target_actions(
        history.actions_in_round(1),
        2,
        round,
        target_player,
        active_player,
    );
    let max_opp_raise_flop = flop_actions
        .iter()
        .copied()
        .filter(|&a| a >= 2)
        .max()
        .unwrap_or(0);

    let turn_actions = collect_target_actions(
        history.actions_in_round(2),
        3,
        round,
        target_player,
        active_player,
    );
    let max_opp_raise_turn = turn_actions
        .iter()
        .copied()
        .filter(|&a| a >= 2)
        .max()
        .unwrap_or(0);

    let river_actions = collect_target_actions(
        history.actions_in_round(3),
        4,
        round,
        target_player,
        active_player,
    );
    let max_opp_raise_river = river_actions
        .iter()
        .copied()
        .filter(|&a| a >= 2)
        .max()
        .unwrap_or(0);

    ReachContext {
        opp_raised_pf,
        opp_called_pf,
        opp_3bet_pf,
        opp_limped_pf,
        max_opp_raise_flop,
        opp_raised_flop: max_opp_raise_flop >= 2,
        opp_called_flop: flop_actions.contains(&1),
        max_opp_raise_turn,
        opp_raised_turn: max_opp_raise_turn >= 2,
        opp_called_turn: turn_actions.contains(&1),
        max_opp_raise_river,
        opp_raised_river: max_opp_raise_river >= 2,
        flop_board_paired: is_prefix_paired(board, 3),
        turn_board_paired: is_prefix_paired(board, 4),
        river_board_paired: is_prefix_paired(board, 5),
        monotone_suit: find_monotone_suit(board),
        has_flop_actions: !history.actions_in_round(1).is_empty(),
        has_turn_actions: !history.actions_in_round(2).is_empty(),
    }
}

fn finalize_distribution(
    combos: Vec<[Card; 2]>,
    weights: Vec<f64>,
    sum_w: f64,
) -> OpponentReachDistribution {
    let mut cdf = Vec::with_capacity(weights.len());
    let mut running = 0.0f64;
    let inv_sum = if sum_w > 1e-12 {
        1.0 / sum_w
    } else {
        1.0 / weights.len() as f64
    };
    for &w in &weights {
        running += w * inv_sum;
        cdf.push(running.min(1.0));
    }
    if let Some(last) = cdf.last_mut() {
        *last = 1.0;
    }

    OpponentReachDistribution {
        combos,
        weights,
        cdf,
        total_weight: sum_w,
    }
}

impl OpponentReachDistribution {
    pub fn build<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        remaining_deck: &[Card],
        board: &[Card],
        round: u8,
        history: &H,
    ) -> Self {
        Self::build_detailed(remaining_deck, board, round, history, None, None)
    }

    pub fn build_with_strategy_map<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        remaining_deck: &[Card],
        board: &[Card],
        round: u8,
        history: &H,
        strategy_map: Option<&HashMap<String, Vec<f64>>>,
    ) -> Self {
        Self::build_detailed(remaining_deck, board, round, history, strategy_map, None)
    }

    pub fn build_detailed<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        remaining_deck: &[Card],
        board: &[Card],
        round: u8,
        history: &H,
        strategy_map: Option<&HashMap<String, Vec<f64>>>,
        my_player: Option<usize>,
    ) -> Self {
        Self::build_with_stats(
            remaining_deck,
            board,
            round,
            history,
            strategy_map,
            my_player,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_with_stats<H: crate::cfr::abstraction::HistoryActions + ?Sized>(
        remaining_deck: &[Card],
        board: &[Card],
        round: u8,
        history: &H,
        strategy_map: Option<&HashMap<String, Vec<f64>>>,
        my_player: Option<usize>,
        opp_vpip: Option<f64>,
        opp_pfr: Option<f64>,
    ) -> Self {
        Self::build_for_target_generic(
            remaining_deck,
            board,
            round,
            history,
            strategy_map,
            my_player.map(|mp| 1 - mp),
            my_player,
            opp_vpip,
            opp_pfr,
        )
    }

    /* Generic target-player reach distribution builder avoiding active-street actor inversion.
     * `target_player`: seat (0=SB, 1=BB) whose hand distribution is built.
     * `active_player`: seat currently about to act at the root node. */
    #[allow(clippy::too_many_arguments)]
    pub fn build_for_target_generic<
        H: crate::cfr::abstraction::HistoryActions + ?Sized,
        V: AsRef<[f64]>,
    >(
        remaining_deck: &[Card],
        board: &[Card],
        round: u8,
        history: &H,
        strategy_map: Option<&HashMap<String, V>>,
        target_player: Option<usize>,
        active_player: Option<usize>,
        opp_vpip: Option<f64>,
        opp_pfr: Option<f64>,
    ) -> Self {
        use crate::cfr::abstraction::{preflop_bucket, preflop_strength_tier};

        let rem_len = remaining_deck.len();
        if rem_len < 2 {
            let dummy = if rem_len == 1 {
                [remaining_deck[0], remaining_deck[0]]
            } else {
                [
                    Card::new(Rank::Ace, Suit::Spades),
                    Card::new(Rank::King, Suit::Spades),
                ]
            };
            return Self {
                combos: vec![dummy],
                weights: vec![1.0],
                cdf: vec![1.0],
                total_weight: 1.0,
            };
        }

        let num_combos = rem_len * (rem_len - 1) / 2;
        let mut combos = Vec::with_capacity(num_combos);
        let mut weights = Vec::with_capacity(num_combos);
        let ctx = extract_reach_context(board, round, history, target_player, active_player);
        let mut sum_w = 0.0f64;

        for i in 0..rem_len {
            for j in (i + 1)..rem_len {
                let cand = [remaining_deck[i], remaining_deck[j]];
                let (pf_idx, pf_name) = preflop_bucket(cand[0], cand[1]);

                let pf_weight =
                    preflop_prior_weight(pf_idx, pf_name, strategy_map, target_player, &ctx);
                let flop_weight = flop_likelihood(&cand, board, round, &ctx);
                let turn_weight = turn_likelihood(&cand, board, round, &ctx);
                let river_weight = river_likelihood(&cand, board, round, &ctx);
                let texture_mult = monotone_texture_multiplier(&cand, &ctx);

                let base_w = (pf_weight * flop_weight * turn_weight * river_weight * texture_mult)
                    .max(1e-5f64);
                let tier = preflop_strength_tier(pf_idx);
                let style_mult = style_width_multiplier(tier, opp_vpip, opp_pfr, ctx.opp_raised_pf);
                let w = (base_w * style_mult).max(1e-5f64);

                combos.push(cand);
                weights.push(w);
                sum_w += w;
            }
        }

        finalize_distribution(combos, weights, sum_w)
    }

    #[inline(always)]
    pub fn sample<R: rand::Rng>(&self, rng: &mut R) -> [Card; 2] {
        if self.combos.is_empty() {
            return [
                Card::new(Rank::Ace, Suit::Spades),
                Card::new(Rank::King, Suit::Spades),
            ];
        }
        if self.combos.len() == 1 {
            return self.combos[0];
        }
        let r: f64 = rng.gen();
        let idx = self
            .cdf
            .partition_point(|&w| w < r)
            .min(self.combos.len() - 1);
        self.combos[idx]
    }

    pub fn top_combos(&self, n: usize) -> Vec<([Card; 2], f64)> {
        let mut indexed: Vec<(usize, f64)> = self.weights.iter().copied().enumerate().collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        indexed
            .into_iter()
            .take(n)
            .map(|(i, w)| {
                (
                    self.combos[i],
                    if self.total_weight > 0.0 {
                        w / self.total_weight
                    } else {
                        0.0
                    },
                )
            })
            .collect()
    }

    pub fn effective_combos(&self) -> usize {
        self.combos.len()
    }
}
