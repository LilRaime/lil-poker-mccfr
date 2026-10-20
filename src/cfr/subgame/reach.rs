/*
 * Explicit Bayesian reach distribution for opponent's private hole cards.
 */

use crate::game::config::*;
use crate::game::holdem::Card;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct OpponentReachDistribution {
    pub combos: Vec<[Card; 2]>,
    pub weights: Vec<f64>,
    pub cdf: Vec<f64>,
    pub total_weight: f64,
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
        use crate::cfr::abstraction::{detect_draws, preflop_bucket, preflop_strength_tier};
        use crate::game::holdem::evaluate_7cards;

        let rem_len = remaining_deck.len();
        if rem_len < 2 {
            let dummy = if rem_len == 1 {
                [remaining_deck[0], remaining_deck[0]]
            } else {
                [
                    Card {
                        rank: crate::game::holdem::Rank::Ace,
                        suit: crate::game::holdem::Suit::Spades,
                    },
                    Card {
                        rank: crate::game::holdem::Rank::King,
                        suit: crate::game::holdem::Suit::Spades,
                    },
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

        let (opp_raised_pf, opp_called_pf, opp_3bet_pf, opp_limped_pf) = if let Some(mp) = my_player
        {
            let opp = 1 - mp;
            let actions = history.actions_in_round(0);
            let opp_actions: Vec<u8> = actions
                .iter()
                .enumerate()
                .filter(|(idx, _)| idx % 2 == opp)
                .map(|(_, &a)| a)
                .collect();
            let opp_raised = opp_actions.iter().any(|&a| a >= RAISE_MIN);
            let opp_called = opp_actions.contains(&CALL_CHECK);

            let mut is_3bet = false;
            for (idx, &a) in actions.iter().enumerate() {
                if idx % 2 == opp && a >= RAISE_MIN {
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

        let (opp_raised_flop, opp_called_flop) = if let Some(mp) = my_player {
            let opp = 1 - mp;
            let actions = history.actions_in_round(1);
            let opp_actions: Vec<u8> = actions
                .iter()
                .enumerate()
                .filter(|(idx, _)| idx % 2 == opp)
                .map(|(_, &a)| a)
                .collect();
            (
                opp_actions.iter().any(|&a| a >= 2),
                opp_actions.contains(&1),
            )
        } else {
            let actions = history.actions_in_round(1);
            (actions.iter().any(|&a| a >= 2), actions.contains(&1))
        };

        let (opp_raised_turn, opp_called_turn) = if let Some(mp) = my_player {
            let opp = 1 - mp;
            let actions = history.actions_in_round(2);
            let opp_actions: Vec<u8> = actions
                .iter()
                .enumerate()
                .filter(|(idx, _)| idx % 2 == opp)
                .map(|(_, &a)| a)
                .collect();
            (
                opp_actions.iter().any(|&a| a >= 2),
                opp_actions.contains(&1),
            )
        } else {
            let actions = history.actions_in_round(2);
            (actions.iter().any(|&a| a >= 2), actions.contains(&1))
        };

        /* Check for monotone / 3-flush board texture across current community cards */
        let mut suit_counts = [0u8; 4];
        for c in board {
            suit_counts[c.suit as usize] += 1;
        }
        let monotone_suit = suit_counts
            .iter()
            .enumerate()
            .find(|(_, &cnt)| cnt >= 3)
            .map(|(s_idx, _)| match s_idx {
                0 => crate::game::holdem::Suit::Clubs,
                1 => crate::game::holdem::Suit::Diamonds,
                2 => crate::game::holdem::Suit::Hearts,
                _ => crate::game::holdem::Suit::Spades,
            });

        let mut sum_w = 0.0f64;

        let compute_pf_fallback_weight = |pf_idx: usize,
                                          opp_3bet: bool,
                                          opp_raised: bool,
                                          opp_called: bool,
                                          opp_limped: bool|
         -> f64 {
            if opp_3bet {
                /* Condensed 3-bet/4-bet range: AA-TT (0..=4), AKs-AQs (13..=14), AKo-AQo (91, 92) */
                let is_tier1 =
                    pf_idx <= 4 || pf_idx == 13 || pf_idx == 14 || pf_idx == 91 || pf_idx == 92;
                let is_tier2 = (5..=7).contains(&pf_idx)
                    || (15..=17).contains(&pf_idx)
                    || pf_idx == 21
                    || pf_idx == 22
                    || pf_idx == 25
                    || pf_idx == 93
                    || pf_idx == 103;
                if is_tier1 {
                    1.00
                } else if is_tier2 {
                    0.65
                } else if pf_idx <= 25 || (91..=95).contains(&pf_idx) {
                    0.20
                } else {
                    0.01 /* Pure junk almost never 3-bets */
                }
            } else if opp_limped {
                /* Capped limping range: monsters rarely limp, mid pairs/connectors limp often */
                if pf_idx <= 2 || pf_idx == 13 || pf_idx == 91 {
                    0.08 /* AA, KK, QQ, AKs, AKo trap rarely */
                } else if pf_idx <= 4 || pf_idx == 14 || pf_idx == 92 {
                    0.25 /* JJ, TT, AQs, AQo rarely limp */
                } else if (5..=12).contains(&pf_idx) || (15..=85).contains(&pf_idx) {
                    1.00 /* 99-22, suited connectors / suited broadways */
                } else if pf_idx <= 130 {
                    0.75 /* Offsuit connectors / broadways */
                } else {
                    0.35 /* Lowest junk */
                }
            } else if opp_raised {
                let tier = preflop_strength_tier(pf_idx);
                match tier {
                    1 | 2 => 1.00,
                    3 => 0.80,
                    4 => 0.30,
                    _ => 0.04,
                }
            } else if opp_called {
                let tier = preflop_strength_tier(pf_idx);
                match tier {
                    1 => 0.35, /* Premium hands rarely just call preflop */
                    2 => 0.60,
                    3 => 0.95,
                    4 => 0.65,
                    _ => 0.18,
                }
            } else {
                let tier = preflop_strength_tier(pf_idx);
                match tier {
                    1 => 0.50,
                    2 => 0.70,
                    3 => 0.85,
                    4 => 0.60,
                    _ => 0.35,
                }
            }
        };

        for i in 0..rem_len {
            for j in (i + 1)..rem_len {
                let cand = [remaining_deck[i], remaining_deck[j]];
                let (pf_idx, _) = preflop_bucket(cand[0], cand[1]);

                /* 1. Preflop Bayesian Prior (informed by blueprint strategy when provided) */
                let pf_weight: f64 = if let Some(strat_map) = strategy_map {
                    let (_, pf_name) = preflop_bucket(cand[0], cand[1]);
                    let pf_key = format!("P:{}/", pf_name);
                    if let Some(probs) = strat_map.get(&pf_key) {
                        if opp_3bet_pf {
                            let r_prob: f64 = probs.iter().skip(RAISE_MIN as usize).sum();
                            r_prob.powi(2).max(0.01)
                        } else if opp_raised_pf {
                            let r_prob: f64 = probs.iter().skip(RAISE_MIN as usize).sum();
                            r_prob.max(0.03)
                        } else if opp_limped_pf {
                            let c_prob = probs.get(CALL_CHECK as usize).copied().unwrap_or(0.5);
                            if pf_idx <= 2 || pf_idx == 13 || pf_idx == 91 {
                                (c_prob * 0.15).max(0.05)
                            } else {
                                c_prob.max(0.20)
                            }
                        } else {
                            probs
                                .get(CALL_CHECK as usize)
                                .copied()
                                .unwrap_or(0.5)
                                .max(0.05)
                        }
                    } else {
                        compute_pf_fallback_weight(
                            pf_idx,
                            opp_3bet_pf,
                            opp_raised_pf,
                            opp_called_pf,
                            opp_limped_pf,
                        )
                    }
                } else {
                    compute_pf_fallback_weight(
                        pf_idx,
                        opp_3bet_pf,
                        opp_raised_pf,
                        opp_called_pf,
                        opp_limped_pf,
                    )
                };

                /* 2. Flop Bayesian Likelihood Update */
                let flop_weight: f64 = if round >= 3 && board.len() >= 3 {
                    let f_score = evaluate_7cards(&cand, &board[..3]);
                    let f_cat = f_score >> 32;
                    let (fd, sd) = detect_draws(&cand, &board[..3]);

                    if opp_raised_flop {
                        /* Opponent bet or raised on flop: polarized to made hands or combo draws */
                        if f_cat >= 2 {
                            1.0 /* Two pair+ */
                        } else if f_cat == 1 {
                            0.85 /* One pair */
                        } else if fd && sd {
                            0.95 /* Combo draw */
                        } else if fd || sd {
                            0.75 /* Single draw */
                        } else {
                            0.08 /* Pure air bluff */
                        }
                    } else if opp_called_flop {
                        /* Opponent check-called on flop: mostly draws, middle pairs, floaters */
                        if f_cat >= 3 {
                            0.45 /* Sets/Straights/Flushes slowplaying */
                        } else if f_cat >= 1 {
                            0.95 /* One pair */
                        } else if fd || sd {
                            0.85 /* Flush/straight draws calling */
                        } else {
                            0.15 /* Floater air */
                        }
                    } else {
                        /* Flop checked through */
                        if f_cat >= 2 {
                            0.30 /* Less likely to check monster */
                        } else if f_cat == 1 {
                            0.80 /* Marginal made hand */
                        } else {
                            0.70 /* Air checking */
                        }
                    }
                } else {
                    1.0
                };

                /* 3. Turn Bayesian Likelihood Update */
                let turn_weight: f64 = if round >= 4 && board.len() >= 4 {
                    let t_score = evaluate_7cards(&cand, &board[..4]);
                    let t_cat = t_score >> 32;
                    let (fd, sd) = detect_draws(&cand, &board[..4]);

                    if opp_raised_turn {
                        /* Double-barrel turn */
                        if t_cat >= 2 {
                            1.0
                        } else if t_cat == 1 {
                            0.70
                        } else if fd || sd {
                            0.65
                        } else {
                            0.05
                        }
                    } else if opp_called_turn {
                        if t_cat >= 2 {
                            0.60
                        } else if t_cat == 1 {
                            0.95
                        } else if fd || sd {
                            0.75
                        } else {
                            0.08
                        }
                    } else {
                        if t_cat >= 2 {
                            0.25
                        } else {
                            0.75
                        }
                    }
                } else {
                    1.0
                };

                /* 4. Monotone Texture Blocker Adjustment */
                let texture_mult: f64 = if let Some(mono_suit) = monotone_suit {
                    let c0_mono = cand[0].suit == mono_suit;
                    let c1_mono = cand[1].suit == mono_suit;
                    let opp_aggressive = opp_raised_flop || opp_raised_turn;

                    if opp_aggressive {
                        if c0_mono && c1_mono {
                            1.45 /* Made flush raising */
                        } else if c0_mono || c1_mono {
                            let mono_card = if c0_mono { cand[0] } else { cand[1] };
                            if mono_card.rank == crate::game::holdem::Rank::Ace {
                                1.35 /* Nut flush draw / blocker */
                            } else if mono_card.rank == crate::game::holdem::Rank::King {
                                1.20 /* 2nd nut flush draw */
                            } else {
                                1.05
                            }
                        } else {
                            /* No flush card and aggressive on monotone board: less likely unless set */
                            0.70
                        }
                    } else if opp_called_flop || opp_called_turn {
                        if c0_mono || c1_mono {
                            1.25 /* Calling with flush draw */
                        } else {
                            0.80 /* Calling without flush draw */
                        }
                    } else {
                        1.00
                    }
                } else {
                    1.00
                };

                let w: f64 = (pf_weight * flop_weight * turn_weight * texture_mult).max(1e-5f64);
                combos.push(cand);
                weights.push(w);
                sum_w += w;
            }
        }

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

        Self {
            combos,
            weights,
            cdf,
            total_weight: sum_w,
        }
    }

    #[inline(always)]
    pub fn sample<R: rand::Rng>(&self, rng: &mut R) -> [Card; 2] {
        if self.combos.is_empty() {
            return [
                Card {
                    rank: crate::game::holdem::Rank::Ace,
                    suit: crate::game::holdem::Suit::Spades,
                },
                Card {
                    rank: crate::game::holdem::Rank::King,
                    suit: crate::game::holdem::Suit::Spades,
                },
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
