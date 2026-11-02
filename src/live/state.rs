/* Live Poker State Snapshot and Round History Tracker */
use serde_json::Value;

use super::protocol::{parse_card_str, parse_json_i32};
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
            let p_chips = parse_json_i32(
                p.get("chips")
                    .or_else(|| p.get("stack"))
                    .or_else(|| p.get("balance"))
                    .or_else(|| p.get("remaining_chips"))
                    .or_else(|| p.get("chip_count")),
            );

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

    if my_chips <= 0 {
        my_chips = parse_json_i32(state.get("my_chips").or_else(|| {
            state
                .get("hero_chips")
                .or_else(|| state.get("stack"))
                .or_else(|| state.get("chips"))
        }));
    }

    let state_bet = parse_json_i32(
        state
            .get("current_bet")
            .or_else(|| state.get("currentBet"))
            .or_else(|| state.get("call_amount")),
    );
    let current_bet = max_bet_on_table.max(state_bet);
    let raw_to_call = (current_bet - my_bet).max(0);
    /* Stack-clamp to_call so effective pot odds reflect actual remaining stack (e.g. 452 chips vs 6210 bet)
     * and subtract uncallable overbet chips from pot (Item F). */
    let to_call = if my_chips > 0 {
        raw_to_call.min(my_chips)
    } else {
        raw_to_call
    };
    let raw_pot = parse_json_i32(state.get("pot"));
    let uncallable = raw_to_call.saturating_sub(to_call);
    let pot = raw_pot.saturating_sub(uncallable).max(0);
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

pub use super::round_tracker::LiveRoundTracker;
