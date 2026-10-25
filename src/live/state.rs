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
    pub big_blind: i32,
    pub small_blind: i32,
    pub my_is_sb: bool,
    pub opp_is_sb: bool,
    pub my_player: usize,
    pub active_opponents: usize,
    pub opp_name: String,
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
    let mut my_is_bb = false;
    let mut opp_is_sb = false;
    let mut opp_name = String::new();
    let mut active_opps = 0usize;
    let mut total_opps = 0usize;
    let mut best_opp_score = i64::MIN;

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
            let is_bb = p
                .get("is_big_blind")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let p_chips = parse_json_i32(p.get("chips"));

            if p_id.eq_ignore_ascii_case(player_id) {
                my_bet = p_bet;
                my_chips = p_chips;
                my_is_sb = is_sb;
                my_is_bb = is_bb;
            } else {
                total_opps += 1;
                let is_folded = p
                    .get("folded")
                    .or_else(|| p.get("is_folded"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                    || p.get("status")
                        .or_else(|| p.get("state"))
                        .and_then(|v| v.as_str())
                        .map(|s| {
                            s.eq_ignore_ascii_case("folded")
                                || s.eq_ignore_ascii_case("out")
                                || s.eq_ignore_ascii_case("sitout")
                        })
                        .unwrap_or(false);

                if !is_folded {
                    active_opps += 1;
                }

                /* Prioritize active opponents with the highest current bet, then highest stack */
                let active_bonus: i64 = if !is_folded { 1_000_000_000 } else { 0 };
                let score = active_bonus + (p_bet as i64) * 100_000 + (p_chips as i64);
                if score >= best_opp_score {
                    best_opp_score = score;
                    opp_bet = p_bet;
                    opp_chips = p_chips;
                    opp_is_sb = is_sb;
                    opp_name = p
                        .get("name")
                        .or_else(|| p.get("username"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(p_id)
                        .to_string();
                }
            }
        }
    }
    let active_opponents = if active_opps > 0 {
        active_opps
    } else {
        total_opps.max(1)
    };

    let state_bet = parse_json_i32(
        state
            .get("current_bet")
            .or_else(|| state.get("currentBet"))
            .or_else(|| state.get("call_amount")),
    );
    let current_bet = max_bet_on_table.max(state_bet);
    let raw_to_call = (current_bet - my_bet).max(0);
    /* Stack-clamp to_call so effective pot odds reflect actual remaining stack (e.g. 452 chips vs 6210 bet) */
    let to_call = if my_chips > 0 {
        raw_to_call.min(my_chips)
    } else {
        raw_to_call
    };
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
        .or_else(|| state.get("hand_count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let big_blind = {
        let bb = parse_json_i32(state.get("big_blind").or_else(|| state.get("bigBlind")));
        if bb > 0 {
            bb
        } else {
            BIG_BLIND
        }
    };
    let small_blind = {
        let sb = parse_json_i32(state.get("small_blind").or_else(|| state.get("smallBlind")));
        if sb > 0 {
            sb
        } else {
            (big_blind / 2).max(1)
        }
    };

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

    /* In Heads-Up (total_opps <= 1): player 0 = SB/Button (IP postflop), player 1 = BB (OOP postflop).
     * In Multi-way (total_opps >= 2): blinds (SB/BB) act first postflop (OOP -> player 1),
     * while non-blind seats act in position (IP -> player 0). */
    let my_player: usize = if total_opps <= 1 {
        if my_is_sb {
            0
        } else if opp_is_sb || my_is_bb {
            1
        } else {
            0
        }
    } else if !board.is_empty() && (my_is_sb || my_is_bb) {
        1
    } else if my_is_sb {
        0
    } else if opp_is_sb || my_is_bb {
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
        big_blind,
        small_blind,
        my_is_sb,
        opp_is_sb,
        my_player,
        active_opponents,
        opp_name,
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
        if b_cards.len() < 3 {
            return;
        }
        let opp_raised_river =
            last_opp_bet > 0 || (river_history.len() == 1 && river_history[0] >= RAISE_MIN);
        let pot_showdown = parse_json_i32(state.get("pot"));

        for p in players {
            let p_id = p
                .get("id")
                .or_else(|| p.get("uuid"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !p_id.eq_ignore_ascii_case(player_id) {
                let mut opp_hole = Vec::new();
                if let Some(cards) = p.get("hole").and_then(|v| v.as_array()) {
                    for c in cards {
                        if let Some(cs) = c.as_str() {
                            if let Some(parsed) = parse_card_str(cs) {
                                opp_hole.push(parsed);
                            }
                        }
                    }
                }
                if opp_hole.len() == 2 {
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
