/*! Live Round and Opponent Action Tracker */
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
    pub last_big_blind: i32,
    pub bot_3bet_preflop: bool,
    pub faced_3bet_recorded_this_hand: bool,
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
            last_big_blind: BIG_BLIND,
            bot_3bet_preflop: false,
            faced_3bet_recorded_this_hand: false,
        }
    }

    pub fn reset_hand(&mut self, hand_id: u64) {
        self.current_hand_id = hand_id;
        self.live_history = [RoundHistory::new(); NUM_ROUNDS];
        self.current_round_idx = 0;
        self.last_opp_bet = 0;
        self.bot_3bet_preflop = false;
        self.faced_3bet_recorded_this_hand = false;
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
                                if s == 0
                                    && self.bot_3bet_preflop
                                    && !self.faced_3bet_recorded_this_hand
                                {
                                    tr.record_facing_3bet(false);
                                    self.faced_3bet_recorded_this_hand = true;
                                }
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

    /** Dynamically infer and track opponent prior actions on current street */
    pub fn infer_and_record_opponent_action(
        &mut self,
        snapshot: &LiveSnapshot,
        tracker: &mut OpponentTracker,
    ) {
        self.infer_opponent_action_inner(snapshot, Some(tracker));
    }

    /** Update live_history only (used in 6-max where LiveTableTracker records per-seat stats) */
    pub fn infer_opponent_history_only(&mut self, snapshot: &LiveSnapshot) {
        self.infer_opponent_action_inner(snapshot, None);
    }

    fn infer_opponent_action_inner(
        &mut self,
        snapshot: &LiveSnapshot,
        mut tracker: Option<&mut OpponentTracker>,
    ) {
        let round_idx = self.current_round_idx;
        let hist = &mut self.live_history[round_idx];
        let mut rng = rand::thread_rng();
        let bb = if snapshot.big_blind > 0 {
            snapshot.big_blind
        } else if self.last_big_blind > 0 {
            self.last_big_blind
        } else {
            BIG_BLIND
        };
        self.last_big_blind = bb;
        let scaled_allin_thresh = (bb * 35).max(1);
        let eff_allin_thresh = if snapshot.opp_chips == 0 && snapshot.opp_bet > 0 {
            snapshot.opp_bet
        } else {
            scaled_allin_thresh
        };
        let eff_limp_thresh = bb.max(1);

        if hist.is_empty() {
            if round_idx == 0 {
                if snapshot.active_opponents >= 2 {
                    /* Multi-way: only record voluntary preflop actions if pot/bet exceeds posted blinds (sb + bb);
                     * otherwise pot is unopened before UTG/MP/CO/BTN/SB so leave preflop history empty! */
                    let blinds_pot = bb + snapshot.small_blind.max(1);
                    if snapshot.current_bet > bb {
                        let opp_bet_amt = snapshot.opp_bet.max(snapshot.current_bet);
                        let raise_act = if opp_bet_amt <= bb * 3 {
                            RAISE_MIN
                        } else {
                            pseudo_harmonic_action_mapping(
                                opp_bet_amt,
                                snapshot.pot,
                                eff_allin_thresh,
                                &mut rng,
                            )
                        };
                        hist.push(raise_act);
                    } else if snapshot.pot > blinds_pot + snapshot.small_blind / 2
                        && !snapshot.my_is_sb
                    {
                        hist.push(CALL_CHECK);
                    }
                } else if !snapshot.my_is_sb {
                    /* Heads-Up: SB acts first. If bot is BB, opponent acted first! */
                    if snapshot.to_call == 0 || snapshot.opp_bet <= eff_limp_thresh {
                        hist.push(CALL_CHECK);
                        if let Some(ref mut tr) = tracker {
                            tr.record_action_street_detailed(
                                CALL_CHECK,
                                1,
                                false,
                                snapshot.opp_bet,
                                snapshot.pot,
                            );
                            tr.record_preflop_positional(true, true, false);
                        }
                    } else {
                        let raise_act = if snapshot.opp_bet <= bb * 3 {
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
                        if let Some(ref mut tr) = tracker {
                            tr.record_action_street_detailed(
                                raise_act,
                                1,
                                false,
                                snapshot.opp_bet,
                                snapshot.pot,
                            );
                            tr.record_preflop_positional(true, true, true);
                        }
                    }
                }
            } else {
                /* Postflop: player 1 acts first (OOP), player 0 acts second (IP).
                 * If Hero is IP (my_player == 0), opponent acted first; if Hero is OOP and faces a bet
                 * (e.g. multi-way seat ahead of Hero bet), record that bet as well. */
                if snapshot.my_player == 0 || snapshot.to_call > 0 {
                    if snapshot.to_call == 0 {
                        hist.push(CALL_CHECK);
                        if let Some(ref mut tr) = tracker {
                            tr.record_action_street_detailed(
                                CALL_CHECK,
                                (round_idx + 1) as u8,
                                false,
                                0,
                                snapshot.pot,
                            );
                        }
                    } else {
                        let bet_amt = snapshot.opp_bet.max(snapshot.to_call);
                        let pre_bet_pot = (snapshot.pot - bet_amt).max(bb);
                        let bet_act = if round_idx >= 1
                            && bet_amt >= bb * 10
                            && (bet_amt as i64) * 10 >= (pre_bet_pot as i64) * 15
                        {
                            ALL_IN
                        } else {
                            pseudo_harmonic_action_mapping(
                                bet_amt,
                                pre_bet_pot,
                                eff_allin_thresh,
                                &mut rng,
                            )
                        };
                        hist.push(bet_act);
                        if let Some(ref mut tr) = tracker {
                            tr.record_action_street_detailed(
                                bet_act,
                                (round_idx + 1) as u8,
                                false,
                                bet_amt,
                                pre_bet_pot,
                            );
                        }
                    }
                }
            }
        } else if snapshot.to_call > 0 {
            /* Facing a bet or re-raise after bot already acted this round */
            let bot_bet_this_round = hist.iter().any(|&a| a >= RAISE_MIN);
            let opp_diff = (snapshot.opp_bet - self.last_opp_bet).max(snapshot.to_call);
            let pre_bet_pot = (snapshot.pot - opp_diff).max(bb);
            let raise_act = if round_idx >= 1
                && opp_diff >= bb * 10
                && (opp_diff as i64) * 10 >= (pre_bet_pot as i64) * 15
            {
                ALL_IN
            } else {
                pseudo_harmonic_action_mapping(opp_diff, pre_bet_pot, eff_allin_thresh, &mut rng)
            };
            hist.push(raise_act);
            let facing_cbet = round_idx == 1 && bot_bet_this_round;
            if let Some(ref mut tr) = tracker {
                tr.record_action_street_detailed(
                    raise_act,
                    (round_idx + 1) as u8,
                    facing_cbet,
                    opp_diff,
                    pre_bet_pot,
                );
                if round_idx == 0 && bot_bet_this_round {
                    tr.record_preflop_positional(snapshot.opp_is_sb, true, true);
                    if self.bot_3bet_preflop && !self.faced_3bet_recorded_this_hand {
                        tr.record_facing_3bet(false);
                        self.faced_3bet_recorded_this_hand = true;
                    }
                }
            }
        }

        if snapshot.my_chips > 0 {
            self.estimated_my_chips = snapshot.my_chips;
        }
        self.last_opp_bet = snapshot.opp_bet;
    }

    pub fn compute_engine_contributions(&self, snapshot: &LiveSnapshot) -> ([i32; 2], u8) {
        let num_players = (snapshot.active_opponents + 1).max(2);
        let sb_floor = if snapshot.small_blind > 0 {
            snapshot.small_blind
        } else {
            (snapshot.big_blind.max(1) / 2).max(1)
        };
        let my_contrib = ((snapshot.pot - snapshot.to_call) / num_players as i32).max(sb_floor);
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

    /** Computes effective-stack scaled contributions for SubgameSolver so STACK_SIZE (10_000)
     * inside TexasHoldemGame preserves the exact real-world Stack-to-Pot Ratio (SPR) and pot odds. */
    pub fn compute_scaled_subgame_contributions(&self, snapshot: &LiveSnapshot) -> ([i32; 2], u8) {
        let (raw_contribs, raises) = self.compute_engine_contributions(snapshot);
        let eff_my_chips = if snapshot.my_chips > 0 {
            snapshot.my_chips
        } else {
            self.estimated_my_chips
        };
        if eff_my_chips <= 0 && snapshot.opp_chips <= 0 {
            return (raw_contribs, raises);
        }
        let my_c = raw_contribs[snapshot.my_player];
        let opp_c = raw_contribs[1 - snapshot.my_player];
        let eff_rem = if snapshot.opp_chips > 0 || snapshot.to_call > 0 {
            eff_my_chips.min(snapshot.opp_chips + snapshot.to_call)
        } else {
            eff_my_chips
        };
        let table_bb = if snapshot.big_blind > 0 {
            snapshot.big_blind
        } else if self.last_big_blind > 0 {
            self.last_big_blind
        } else {
            BIG_BLIND
        };
        let max_c = my_c.max(opp_c);
        let eff_total = (max_c + eff_rem.saturating_sub(opp_c.saturating_sub(my_c)))
            .max(max_c)
            .max(table_bb * 2);
        let scale = STACK_SIZE as f64 / eff_total as f64;
        let mut scaled_my = ((my_c as f64 * scale).round() as i32).clamp(SMALL_BLIND, STACK_SIZE);
        let mut scaled_opp = ((opp_c as f64 * scale).round() as i32).clamp(SMALL_BLIND, STACK_SIZE);
        if opp_c > my_c && scaled_opp <= scaled_my {
            if scaled_opp < STACK_SIZE {
                scaled_opp = scaled_my + 1;
            } else {
                scaled_my = (STACK_SIZE - 1).max(SMALL_BLIND);
            }
        }
        let scaled = if snapshot.my_player == 0 {
            [scaled_my, scaled_opp]
        } else {
            [scaled_opp, scaled_my]
        };
        (scaled, raises)
    }

    pub fn record_bot_action(&mut self, chosen_action: &str, amount: i32, pot: i32) {
        self.record_bot_action_with_bet(chosen_action, amount, pot, 0);
    }

    pub fn record_bot_action_with_bet(
        &mut self,
        chosen_action: &str,
        amount: i32,
        pot: i32,
        my_bet: i32,
    ) {
        let bb = if self.last_big_blind > 0 {
            self.last_big_blind
        } else {
            BIG_BLIND
        };
        let sb = (bb / 2).max(1);
        let allin_thresh = if self.estimated_my_chips > 0 {
            self.estimated_my_chips.min(bb * 35).max(bb * 2)
        } else {
            (bb * 35).max(1)
        };
        let open_min_total = (bb * 5 / 2).max(bb * 2);
        let added = (amount - my_bet).max(0);
        let bot_act = match chosen_action {
            "fold" => FOLD,
            "check" | "call" => CALL_CHECK,
            "raise" | "bet" => {
                if added > 0 {
                    self.estimated_my_chips = self.estimated_my_chips.saturating_sub(added);
                }
                if added >= allin_thresh {
                    ALL_IN
                } else if self.current_round_idx == 0 {
                    if amount == open_min_total && pot <= sb + bb {
                        RAISE_MIN
                    } else if added >= pot / 2 {
                        RAISE_HALF_POT
                    } else if added >= pot / 3 {
                        RAISE_THIRD_POT
                    } else {
                        RAISE_MIN
                    }
                } else {
                    let half_thresh = (pot / 2).max(bb);
                    let third_thresh = (pot / 3).max(bb);
                    if added >= half_thresh {
                        RAISE_HALF_POT
                    } else if added >= third_thresh {
                        RAISE_THIRD_POT
                    } else {
                        RAISE_MIN
                    }
                }
            }
            "allin" | "all_in" => ALL_IN,
            _ => CALL_CHECK,
        };
        if self.current_round_idx == 0
            && bot_act >= RAISE_MIN
            && self.live_history[0].iter().any(|&a| a >= RAISE_MIN)
        {
            self.bot_3bet_preflop = true;
        }
        self.live_history[self.current_round_idx].push(bot_act);
    }
}
