use serde_json::Value;

use super::{
    extract_blind_flags, extract_player_bet, extract_player_chips, extract_player_id,
    extract_player_name, is_player_folded, parse_cards_array, resolve_blinds,
};
use crate::game::config::STACK_SIZE;
use crate::game::holdem::Card;
use crate::live::protocol::parse_json_i32;

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

#[derive(Default)]
struct TablePlayersSummary {
    max_bet_on_table: i32,
    my_bet: i32,
    opp_bet: i32,
    my_chips: i32,
    opp_chips: i32,
    my_is_sb: bool,
    my_is_bb: bool,
    opp_is_sb: bool,
    sb_posted_bet: i32,
    bb_posted_bet: i32,
    opp_name: String,
    active_opps: usize,
    total_opps: usize,
    opp_bets: Vec<i32>,
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

fn summarize_players(players: &[Value], player_id: &str) -> TablePlayersSummary {
    let mut sum = TablePlayersSummary::default();
    let mut best_opp_score = i64::MIN;

    for p in players {
        let p_bet = extract_player_bet(p);
        if p_bet > sum.max_bet_on_table {
            sum.max_bet_on_table = p_bet;
        }
        let p_id = extract_player_id(p);
        let (is_sb, is_bb) = extract_blind_flags(p);
        if is_sb && p_bet > 0 {
            sum.sb_posted_bet = p_bet;
        }
        if is_bb && p_bet > 0 {
            sum.bb_posted_bet = p_bet;
        }
        let p_chips = extract_player_chips(p);

        if p_id.eq_ignore_ascii_case(player_id) {
            sum.my_bet = p_bet;
            sum.my_chips = p_chips;
            sum.my_is_sb = is_sb;
            sum.my_is_bb = is_bb;
        } else {
            sum.total_opps += 1;
            sum.opp_bets.push(p_bet);
            let is_folded = is_player_folded(p);
            if !is_folded {
                sum.active_opps += 1;
            }
            /* Prioritize active opponents with the highest current bet, then highest stack */
            let active_bonus: i64 = if !is_folded { 1_000_000_000 } else { 0 };
            let score = active_bonus + (p_bet as i64) * 100_000 + (p_chips as i64);
            if score >= best_opp_score {
                best_opp_score = score;
                sum.opp_bet = p_bet;
                sum.opp_chips = p_chips;
                sum.opp_is_sb = is_sb;
                sum.opp_name = extract_player_name(p, p_id).to_string();
            }
        }
    }
    sum
}

fn compute_pot_and_to_call(
    state: &Value,
    sum: &TablePlayersSummary,
    my_chips: i32,
) -> (i32, i32, i32) {
    let state_bet = parse_json_i32(
        state
            .get("current_bet")
            .or_else(|| state.get("currentBet"))
            .or_else(|| state.get("call_amount")),
    );
    let current_bet = sum.max_bet_on_table.max(state_bet);
    let raw_to_call = (current_bet - sum.my_bet).max(0);
    /* Stack-clamp to_call so effective pot odds reflect actual remaining stack (e.g. 452 chips vs 6210 bet)
     * and subtract uncallable overbet chips across ALL opponents from pot without wiping prior pot. */
    let to_call = if my_chips > 0 {
        raw_to_call.min(my_chips)
    } else {
        raw_to_call
    };
    let raw_pot = parse_json_i32(state.get("pot"));
    let single_uncallable = raw_to_call.saturating_sub(to_call);
    let total_uncallable = if my_chips > 0 {
        let my_max_street_bet = sum.my_bet + my_chips;
        let multi_uncallable: i32 = sum
            .opp_bets
            .iter()
            .filter(|&&b| b > my_max_street_bet)
            .map(|&b| b - my_max_street_bet)
            .sum();
        multi_uncallable.max(single_uncallable)
    } else {
        single_uncallable
    };
    let pot = if raw_pot > total_uncallable + to_call {
        raw_pot - total_uncallable
    } else {
        raw_pot.max(to_call)
    };
    (current_bet, pot, to_call)
}

fn compute_hero_position(board_empty: bool, sum: &TablePlayersSummary) -> usize {
    /* In Heads-Up (total_opps <= 1): player 0 = SB/Button (IP postflop), player 1 = BB (OOP postflop).
     * In Multi-way (total_opps >= 2): blinds (SB/BB) act first postflop (OOP -> player 1),
     * while non-blind seats act in position (IP -> player 0). */
    if sum.total_opps <= 1 {
        if sum.my_is_sb {
            0
        } else if sum.opp_is_sb || sum.my_is_bb {
            1
        } else {
            0
        }
    } else if !board_empty && (sum.my_is_sb || sum.my_is_bb) {
        1
    } else if sum.my_is_sb {
        0
    } else if sum.opp_is_sb || sum.my_is_bb {
        1
    } else {
        0
    }
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

    let players = state.get("players").and_then(|v| v.as_array());
    let mut hole_cards = Vec::new();
    if let Some(arr) = players {
        for p in arr {
            if extract_player_id(p).eq_ignore_ascii_case(player_id) {
                hole_cards.extend(parse_cards_array(p.get("hole")));
            }
        }
    }
    if hole_cards.len() < 2 {
        return None;
    }
    let hole = [hole_cards[0], hole_cards[1]];
    let board = parse_cards_array(state.get("board"));

    let mut sum = players
        .map(|arr| summarize_players(arr, player_id))
        .unwrap_or_default();

    let active_opponents = if sum.active_opps > 0 {
        sum.active_opps
    } else {
        sum.total_opps.max(1)
    };

    if sum.my_chips <= 0 {
        sum.my_chips = parse_json_i32(state.get("my_chips").or_else(|| {
            state
                .get("hero_chips")
                .or_else(|| state.get("stack"))
                .or_else(|| state.get("chips"))
        }));
    }

    let (current_bet, pot, to_call) = compute_pot_and_to_call(state, &sum, sum.my_chips);
    let phase = state
        .get("phase")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let effective_my_chips = if sum.my_chips > 0 {
        sum.my_chips
    } else {
        STACK_SIZE
    };

    /* Extract legal actions or dynamically derive them from table state */
    let legal_actions: Vec<String> = state
        .get("legal_actions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                .collect::<Vec<_>>()
        })
        .filter(|parsed| !parsed.is_empty())
        .unwrap_or_else(|| derive_legal_actions(to_call, effective_my_chips));

    let hand_id = state
        .get("hand_number")
        .or_else(|| state.get("hand_id"))
        .or_else(|| state.get("hand_count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let (big_blind, small_blind) = resolve_blinds(
        state,
        board.is_empty(),
        sum.sb_posted_bet,
        sum.bb_posted_bet,
    );

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
        sum.my_bet,
        pot,
        hist_len
    );

    let my_player = compute_hero_position(board.is_empty(), &sum);

    Some(LiveSnapshot {
        hand_id,
        phase,
        hole,
        board,
        legal_actions,
        current_bet,
        my_bet: sum.my_bet,
        opp_bet: sum.opp_bet,
        my_chips: sum.my_chips,
        opp_chips: sum.opp_chips,
        pot,
        to_call,
        big_blind,
        small_blind,
        my_is_sb: sum.my_is_sb,
        opp_is_sb: sum.opp_is_sb,
        my_player,
        active_opponents,
        opp_name: sum.opp_name,
        action_seq_key,
    })
}
