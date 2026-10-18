/* Live Poker State Snapshot and Round History Tracker */
use serde_json::Value;

use super::protocol::{parse_card_str, parse_json_i32, pseudo_harmonic_action_mapping};
use crate::cfr::opponent_model::OpponentTracker;
use crate::game::config::*;
use crate::game::holdem::{Card, RoundHistory};

/* Parsed snapshot of live poker table state for bot decision */
#[derive(Debug, Clone)]
pub struct LiveSnapshot {
    pub hand_id: u64,
    pub phase: String,
    pub hole: [Card; 2],
    pub board: Vec<Card>,
    pub legal_actions: Vec<String>,
    pub current_bet: i32,
    pub my_bet: i32,
    pub opp_bet: i32,
    pub my_chips: i32,
    pub opp_chips: i32,
    pub pot: i32,
    pub to_call: i32,
    pub my_is_sb: bool,
    pub opp_is_sb: bool,
    pub my_player: usize,
    pub action_seq_key: String,
}

/* Dynamically derive legal action tokens when server does not provide legal_actions */
pub fn derive_legal_actions(to_call: i32, my_chips: i32) -> Vec<String> {
    let mut actions = Vec::new();
    if to_call == 0 {
        actions.push("check".to_string());
        if my_chips > 0 {
            actions.push("raise".to_string());
            actions.push("bet".to_string());
            actions.push("all_in".to_string());
            actions.push("allin".to_string());
        }
    } else {
        actions.push("fold".to_string());
        actions.push("call".to_string());
        if my_chips > to_call {
            actions.push("raise".to_string());
            actions.push("bet".to_string());
            actions.push("all_in".to_string());
            actions.push("allin".to_string());
        } else if my_chips > 0 {
            actions.push("all_in".to_string());
            actions.push("allin".to_string());
        }
    }
    actions
}

/* Parse a WebSocket JSON message into LiveSnapshot if bot is active player */
pub fn parse_live_state(state: &Value, player_id: &str) -> Option<LiveSnapshot> {
    let active_id = state
        .get("active_player_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !active_id.eq_ignore_ascii_case(player_id) {
        return None;
    }

    /* Extract player hole cards */
    let mut hole_cards = Vec::new();
    if let Some(players) = state.get("players").and_then(|v| v.as_array()) {
        for p in players {
            let p_id = p
                .get("id")
                .or_else(|| p.get("uuid"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if p_id.eq_ignore_ascii_case(player_id) {
                if let Some(cards) = p.get("hole").and_then(|v| v.as_array()) {
                    for c in cards {
                        if let Some(cs) = c.as_str() {
                            if let Some(parsed) = parse_card_str(cs) {
                                hole_cards.push(parsed);
                            }
                        }
                    }
                }
            }
        }
    }

    if hole_cards.len() < 2 {
        return None;
    }
    let hole = [hole_cards[0], hole_cards[1]];

    /* Extract board cards */
    let mut board = Vec::new();
    if let Some(b) = state.get("board").and_then(|v| v.as_array()) {
        for c in b {
            if let Some(cs) = c.as_str() {
                if let Some(parsed) = parse_card_str(cs) {
                    board.push(parsed);
                }
            }
        }
    }

    let mut max_bet_on_table = 0i32;
    let mut my_bet = 0i32;
    let mut opp_bet = 0i32;
    let mut my_chips = 0i32;
    let mut opp_chips = 0i32;
    let mut my_is_sb = false;
    let mut opp_is_sb = false;

    if let Some(players) = state.get("players").and_then(|v| v.as_array()) {
        for p in players {
            let p_bet = parse_json_i32(p.get("bet").or_else(|| p.get("current_bet")));
            if p_bet > max_bet_on_table {
                max_bet_on_table = p_bet;
            }

            let p_id = p
                .get("id")
                .or_else(|| p.get("uuid"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let is_sb = p
                .get("is_small_blind")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let p_chips = parse_json_i32(p.get("chips"));

            if p_id.eq_ignore_ascii_case(player_id) {
                my_bet = p_bet;
                my_chips = p_chips;
                my_is_sb = is_sb;
            } else {
                opp_bet = p_bet;
                opp_chips = p_chips;
                opp_is_sb = is_sb;
            }
        }
    }

    let state_bet = parse_json_i32(
        state
            .get("current_bet")
            .or_else(|| state.get("currentBet"))
            .or_else(|| state.get("call_amount")),
    );
    let current_bet = max_bet_on_table.max(state_bet);
    let to_call = (current_bet - my_bet).max(0);
    let pot = parse_json_i32(state.get("pot"));
    let phase = state
        .get("phase")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let effective_my_chips = if my_chips > 0 { my_chips } else { STACK_SIZE };

    /* Extract legal actions or dynamically derive them from table state */
    let legal_actions: Vec<String> =
        if let Some(arr) = state.get("legal_actions").and_then(|v| v.as_array()) {
            let parsed: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                .collect();
            if !parsed.is_empty() {
                parsed
            } else {
                derive_legal_actions(to_call, effective_my_chips)
            }
        } else {
            derive_legal_actions(to_call, effective_my_chips)
        };

    let hand_id = state
        .get("hand_number")
        .or_else(|| state.get("hand_id"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let hist_len = state
        .get("history")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let action_seq_key = format!(
        "{}:{}:{}:{}:{}:{}:{}",
        hand_id,
        phase,
        board.len(),
        current_bet,
        my_bet,
        pot,
        hist_len
    );

    let my_player: usize = if my_is_sb {
        0
    } else if opp_is_sb {
        1
    } else {
        0
    };

    Some(LiveSnapshot {
        hand_id,
        phase,
        hole,
        board,
        legal_actions,
        current_bet,
        my_bet,
        opp_bet,
        my_chips,
        opp_chips,
        pot,
        to_call,
        my_is_sb,
        opp_is_sb,
        my_player,
        action_seq_key,
    })
}

/* Process showdown event: inspect opponent cards and record to OpponentTracker */
pub fn process_showdown(
    state: &Value,
    player_id: &str,
    tracker: &mut OpponentTracker,
    river_history: &RoundHistory,
    last_opp_bet: i32,
) {
    if let Some(players) = state.get("players").and_then(|v| v.as_array()) {
        let mut opp_hole = Vec::new();
        for p in players {
            let p_id = p
                .get("id")
                .or_else(|| p.get("uuid"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !p_id.eq_ignore_ascii_case(player_id) {
                if let Some(cards) = p.get("hole").and_then(|v| v.as_array()) {
                    for c in cards {
                        if let Some(cs) = c.as_str() {
                            if let Some(parsed) = parse_card_str(cs) {
                                opp_hole.push(parsed);
                            }
                        }
                    }
                }
            }
        }
        let mut b_cards = Vec::new();
        if let Some(board) = state.get("board").and_then(|v| v.as_array()) {
            for c in board {
                if let Some(cs) = c.as_str() {
                    if let Some(parsed) = parse_card_str(cs) {
                        b_cards.push(parsed);
                    }
                }
            }
        }
        if opp_hole.len() == 2 && b_cards.len() >= 3 {
            let opp_raised_river = river_history.iter().any(|&a| a >= RAISE_MIN);
            let pot_showdown = parse_json_i32(state.get("pot"));
            tracker.record_showdown_hand_detailed(
                [opp_hole[0], opp_hole[1]],
                &b_cards,
                opp_raised_river,
                last_opp_bet,
                pot_showdown,
            );
        }
    }
}

/* Live Round & History Tracker */
#[derive(Debug, Clone)]
pub struct LiveRoundTracker {
    pub current_hand_id: u64,
    pub current_round_idx: usize,
    pub live_history: [RoundHistory; NUM_ROUNDS],
    pub last_opp_bet: i32,
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
        }
    }

    pub fn reset_hand(&mut self, hand_id: u64) {
        self.current_hand_id = hand_id;
        self.live_history = [RoundHistory::new(); NUM_ROUNDS];
        self.current_round_idx = 0;
        self.last_opp_bet = 0;
    }

    pub fn update_street(&mut self, board_len: usize) -> usize {
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
                        } else if self.live_history[s].len() == 1 && last_act == CALL_CHECK {
                            /* Single limp / call on street was checked by opponent to advance */
                            self.live_history[s].push(CALL_CHECK);
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

        if hist.is_empty() {
            if round_idx == 0 {
                /* Preflop: SB acts first. If bot is BB, opponent acted first! */
                if !snapshot.my_is_sb {
                    if snapshot.to_call == 0 || snapshot.opp_bet <= LIMP_THRESHOLD {
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
                        let raise_act = pseudo_harmonic_action_mapping(
                            snapshot.opp_bet,
                            snapshot.pot,
                            ALLIN_BET_THRESHOLD,
                            &mut rng,
                        );
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
                            ALLIN_BET_THRESHOLD,
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
            /* Facing a re-raise after bot already acted this round */
            let opp_diff = (snapshot.opp_bet - self.last_opp_bet).max(snapshot.to_call);
            let raise_act = pseudo_harmonic_action_mapping(
                opp_diff,
                snapshot.pot,
                ALLIN_BET_THRESHOLD,
                &mut rng,
            );
            hist.push(raise_act);
            let facing_cbet = round_idx == 1;
            tracker.record_action_street_detailed(
                raise_act,
                (round_idx + 1) as u8,
                facing_cbet,
                opp_diff,
                snapshot.pot,
            );
        }

        self.last_opp_bet = snapshot.opp_bet;
    }

    pub fn compute_engine_contributions(&self, snapshot: &LiveSnapshot) -> ([i32; 2], u8) {
        let my_contrib = ((snapshot.pot - snapshot.to_call) / 2).max(SMALL_BLIND);
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

    pub fn record_bot_action(&mut self, chosen_action: &str, amount: i32, pot: i32) {
        let bot_act = match chosen_action {
            "fold" => FOLD,
            "check" | "call" => CALL_CHECK,
            "raise" | "bet" => {
                if amount >= ALLIN_BET_THRESHOLD {
                    ALL_IN
                } else if amount >= pot / 2 {
                    RAISE_HALF_POT
                } else if amount >= pot / 3 {
                    RAISE_THIRD_POT
                } else {
                    RAISE_MIN
                }
            }
            "allin" | "all_in" => ALL_IN,
            _ => CALL_CHECK,
        };
        self.live_history[self.current_round_idx].push(bot_act);
    }
}
