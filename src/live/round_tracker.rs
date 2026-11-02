/* Live Round and Opponent Action Tracker */
use super::protocol::pseudo_harmonic_action_mapping;
use super::state::LiveSnapshot;
use crate::cfr::opponent_model::OpponentTracker;
use crate::game::config::*;
use crate::game::holdem::{
    RoundHistory, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};

#[derive(Debug, Clone)]
pub struct LiveRoundTracker {
    pub current_hand_id: u64,
    pub current_round_idx: usize,
    pub live_history: [RoundHistory; NUM_ROUNDS],
    pub last_opp_bet: i32,
    pub estimated_my_chips: i32,
}

impl Default for LiveRoundTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveRoundTracker {
    pub fn new() -> Self {
        LiveRoundTracker {
            current_hand_id: 0,
            current_round_idx: 0,
            live_history: [RoundHistory::new(); NUM_ROUNDS],
            last_opp_bet: 0,
            estimated_my_chips: STACK_SIZE,
        }
    }

    pub fn reset_hand(&mut self, hand_id: u64) {
        self.current_hand_id = hand_id;
        self.live_history = [RoundHistory::new(); NUM_ROUNDS];
        self.current_round_idx = 0;
        self.last_opp_bet = 0;
    }

    pub fn update_street(&mut self, board_len: usize) -> usize {
        self.update_street_with_tracker(board_len, None)
    }

    pub fn update_street_with_tracker(
        &mut self,
        board_len: usize,
        mut tracker: Option<&mut OpponentTracker>,
    ) -> usize {
        let round_idx = match board_len {
            0 => 0,
            3 => 1,
            4 => 2,
            _ => 3,
        };
        if round_idx != self.current_round_idx {
            if round_idx > self.current_round_idx {
                for s in self.current_round_idx..round_idx {
                    if self.live_history[s].is_empty() {
                        /* Previous street unrecorded; default to standard sequence so rich keys match */
                        if s == 0 {
                            self.live_history[s].push(RAISE_MIN);
                            self.live_history[s].push(CALL_CHECK);
                        } else {
                            self.live_history[s].push(CALL_CHECK);
                            self.live_history[s].push(CALL_CHECK);
                        }
                    } else if let Some(&last_act) = self.live_history[s].last() {
                        if last_act >= RAISE_MIN {
                            /* Street ended on a bet/raise that was called to advance to next street */
                            self.live_history[s].push(CALL_CHECK);
                            if let Some(ref mut tr) = tracker {
                                tr.record_action_street_detailed(
                                    CALL_CHECK,
                                    (s + 1) as u8,
                                    s >= 1,
                                    0,
                                    0,
                                );
                            }
                        } else if self.live_history[s].len() == 1 && last_act == CALL_CHECK {
                            /* Single limp / call on street was checked by opponent to advance */
                            self.live_history[s].push(CALL_CHECK);
                            if let Some(ref mut tr) = tracker {
                                tr.record_action_street_detailed(
                                    CALL_CHECK,
                                    (s + 1) as u8,
                                    false,
                                    0,
                                    0,
                                );
                            }
                        }
                    }
                }
            }
            self.current_round_idx = round_idx;
            self.last_opp_bet = 0;
        }
        round_idx
    }

    /* Dynamically infer and track opponent prior actions on current street */
    pub fn infer_and_record_opponent_action(
        &mut self,
        snapshot: &LiveSnapshot,
        tracker: &mut OpponentTracker,
    ) {
        let round_idx = self.current_round_idx;
        let hist = &mut self.live_history[round_idx];
        let mut rng = rand::thread_rng();
        let eff_allin_thresh = if snapshot.opp_chips == 0 && snapshot.opp_bet > 0 {
            snapshot.opp_bet
        } else {
            ALLIN_BET_THRESHOLD
        };
        let eff_limp_thresh = snapshot.big_blind.max(LIMP_THRESHOLD);

        if hist.is_empty() {
            if round_idx == 0 {
                /* Preflop: SB acts first. If bot is BB, opponent acted first! */
                if !snapshot.my_is_sb {
                    if snapshot.to_call == 0 || snapshot.opp_bet <= eff_limp_thresh {
                        hist.push(CALL_CHECK);
                        tracker.record_action_street_detailed(
                            CALL_CHECK,
                            1,
                            false,
                            snapshot.opp_bet,
                            snapshot.pot,
                        );
                        tracker.record_preflop_positional(true, true, false);
                    } else {
                        let raise_act = if snapshot.opp_bet <= snapshot.big_blind * 3 {
                            RAISE_MIN
                        } else {
                            pseudo_harmonic_action_mapping(
                                snapshot.opp_bet,
                                snapshot.pot,
                                eff_allin_thresh,
                                &mut rng,
                            )
                        };
                        hist.push(raise_act);
                        tracker.record_action_street_detailed(
                            raise_act,
                            1,
                            false,
                            snapshot.opp_bet,
                            snapshot.pot,
                        );
                        tracker.record_preflop_positional(true, true, true);
                    }
                }
            } else {
                /* Postflop: BB acts first. If opponent is BB (OOP), opponent acted first! */
                if snapshot.my_is_sb && !snapshot.opp_is_sb {
                    if snapshot.to_call == 0 {
                        hist.push(CALL_CHECK);
                        tracker.record_action_street_detailed(
                            CALL_CHECK,
                            (round_idx + 1) as u8,
                            false,
                            0,
                            snapshot.pot,
                        );
                    } else {
                        let bet_act = pseudo_harmonic_action_mapping(
                            snapshot.opp_bet,
                            snapshot.pot,
                            eff_allin_thresh,
                            &mut rng,
                        );
                        hist.push(bet_act);
                        tracker.record_action_street_detailed(
                            bet_act,
                            (round_idx + 1) as u8,
                            false,
                            snapshot.opp_bet,
                            snapshot.pot,
                        );
                    }
                }
            }
        } else if snapshot.to_call > 0 {
            /* Facing a bet or re-raise after bot already acted this round */
            let bot_bet_this_round = hist.iter().any(|&a| a >= RAISE_MIN);
            let opp_diff = (snapshot.opp_bet - self.last_opp_bet).max(snapshot.to_call);
            let raise_act =
                pseudo_harmonic_action_mapping(opp_diff, snapshot.pot, eff_allin_thresh, &mut rng);
            hist.push(raise_act);
            let facing_cbet = round_idx == 1 && bot_bet_this_round;
            tracker.record_action_street_detailed(
                raise_act,
                (round_idx + 1) as u8,
                facing_cbet,
                opp_diff,
                snapshot.pot,
            );
            if round_idx == 0 && bot_bet_this_round {
                tracker.record_preflop_positional(snapshot.opp_is_sb, true, true);
            }
        }

        if snapshot.my_chips > 0 {
            self.estimated_my_chips = snapshot.my_chips;
        }
        self.last_opp_bet = snapshot.opp_bet;
    }

    pub fn compute_engine_contributions(&self, snapshot: &LiveSnapshot) -> ([i32; 2], u8) {
        let num_players = (snapshot.active_opponents + 1).max(2);
        let my_contrib = ((snapshot.pot - snapshot.to_call) / num_players as i32).max(SMALL_BLIND);
        let opp_contrib = my_contrib + snapshot.to_call;
        let engine_contributions = if snapshot.my_player == 0 {
            [my_contrib, opp_contrib]
        } else {
            [opp_contrib, my_contrib]
        };

        let raises_this_round = self.live_history[self.current_round_idx]
            .iter()
            .filter(|&&a| a >= RAISE_MIN)
            .count() as u8;

        (engine_contributions, raises_this_round)
    }

    /* Computes effective-stack scaled contributions for SubgameSolver so STACK_SIZE (10_000)
     * inside TexasHoldemGame preserves the exact real-world Stack-to-Pot Ratio (SPR) and pot odds. */
    pub fn compute_scaled_subgame_contributions(&self, snapshot: &LiveSnapshot) -> ([i32; 2], u8) {
        let (raw_contribs, raises) = self.compute_engine_contributions(snapshot);
        if snapshot.my_chips <= 0 && snapshot.opp_chips <= 0 {
            return (raw_contribs, raises);
        }
        let my_c = raw_contribs[snapshot.my_player];
        let opp_c = raw_contribs[1 - snapshot.my_player];
        let eff_rem = if snapshot.opp_chips > 0 || snapshot.to_call > 0 {
            snapshot.my_chips.min(snapshot.opp_chips + snapshot.to_call)
        } else {
            snapshot.my_chips
        };
        let eff_total = (my_c + eff_rem).max(opp_c).max(BIG_BLIND * 2);
        let scale = STACK_SIZE as f64 / eff_total as f64;
        let scaled_my = ((my_c as f64 * scale).round() as i32).clamp(SMALL_BLIND, STACK_SIZE);
        let scaled_opp = ((opp_c as f64 * scale).round() as i32).clamp(SMALL_BLIND, STACK_SIZE);
        let scaled = if snapshot.my_player == 0 {
            [scaled_my, scaled_opp]
        } else {
            [scaled_opp, scaled_my]
        };
        (scaled, raises)
    }

    pub fn record_bot_action(&mut self, chosen_action: &str, amount: i32, pot: i32) {
        let bot_act = match chosen_action {
            "fold" => FOLD,
            "check" | "call" => CALL_CHECK,
            "raise" | "bet" => {
                if amount >= ALLIN_BET_THRESHOLD {
                    ALL_IN
                } else if self.current_round_idx == 0 {
                    if amount == PREFLOP_OPEN_MIN_TOTAL && pot <= SMALL_BLIND + BIG_BLIND {
                        RAISE_MIN
                    } else if amount >= pot / 2 {
                        RAISE_HALF_POT
                    } else if amount >= pot / 3 {
                        RAISE_THIRD_POT
                    } else {
                        RAISE_MIN
                    }
                } else {
                    let half_thresh = (pot / 2).max(RAISE_HALF_POT_FLOOR_DRY);
                    let third_thresh = (pot / 3).max(RAISE_THIRD_POT_FLOOR);
                    if amount >= half_thresh {
                        RAISE_HALF_POT
                    } else if amount >= third_thresh {
                        RAISE_THIRD_POT
                    } else {
                        RAISE_MIN
                    }
                }
            }
            "allin" | "all_in" => ALL_IN,
            _ => CALL_CHECK,
        };
        self.live_history[self.current_round_idx].push(bot_act);
    }
}
