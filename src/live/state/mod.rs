/* Live Poker State Snapshot and Table/Round History Tracker */
pub mod snapshot;
pub mod table_tracker;

pub use snapshot::{
    derive_legal_actions, parse_live_state, parse_live_state_with_hint, LiveSnapshot,
};
pub use table_tracker::{
    process_showdown, process_showdown_multiway, LiveTableTracker, OppHandState,
};

pub use super::round_tracker::LiveRoundTracker;

use serde_json::Value;

use super::protocol::{parse_card_str, parse_json_i32};
use crate::game::config::BIG_BLIND;
use crate::game::holdem::Card;

pub(super) fn extract_player_id(p: &Value) -> &str {
    p.get("id")
        .or_else(|| p.get("uuid"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

pub(super) fn extract_player_name<'a>(p: &'a Value, fallback_id: &'a str) -> &'a str {
    p.get("name")
        .or_else(|| p.get("username"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_id)
}

pub(super) fn extract_player_bet(p: &Value) -> i32 {
    parse_json_i32(p.get("bet").or_else(|| p.get("current_bet")))
}

pub(super) fn extract_player_chips(p: &Value) -> i32 {
    parse_json_i32(
        p.get("chips")
            .or_else(|| p.get("stack"))
            .or_else(|| p.get("balance"))
            .or_else(|| p.get("remaining_chips"))
            .or_else(|| p.get("chip_count")),
    )
}

pub(super) fn extract_blind_flags(p: &Value) -> (bool, bool) {
    let is_sb = p
        .get("is_small_blind")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let is_bb = p
        .get("is_big_blind")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    (is_sb, is_bb)
}

pub(super) fn parse_cards_array(val: Option<&Value>) -> Vec<Card> {
    let mut cards = Vec::new();
    if let Some(arr) = val.and_then(|v| v.as_array()) {
        for c in arr {
            if let Some(cs) = c.as_str() {
                if let Some(parsed) = parse_card_str(cs) {
                    cards.push(parsed);
                }
            }
        }
    }
    cards
}

pub(super) fn is_player_folded(p: &Value) -> bool {
    p.get("folded")
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
            .unwrap_or(false)
}

pub(super) fn parse_raw_blinds(state: &Value) -> (i32, i32) {
    let bb = parse_json_i32(
        state
            .get("big_blind")
            .or_else(|| state.get("bigBlind"))
            .or_else(|| state.get("bb"))
            .or_else(|| state.get("min_bet"))
            .or_else(|| state.get("minBet")),
    );
    let sb = parse_json_i32(
        state
            .get("small_blind")
            .or_else(|| state.get("smallBlind"))
            .or_else(|| state.get("sb")),
    );
    (bb, sb)
}

pub(super) fn resolve_blinds(
    state: &Value,
    board_empty: bool,
    sb_posted_bet: i32,
    bb_posted_bet: i32,
    nonzero_bets: &[i32],
    raw_pot: i32,
    hint_bb: i32,
) -> (i32, i32) {
    let (raw_bb, raw_sb) = parse_raw_blinds(state);
    let inferred_from_table = if board_empty && nonzero_bets.len() == 2 {
        let b0 = nonzero_bets[0];
        let b1 = nonzero_bets[1];
        let (min_b, max_b) = (b0.min(b1), b0.max(b1));
        if min_b > 0 && max_b == min_b * 2 && raw_pot == min_b + max_b {
            max_b
        } else {
            0
        }
    } else {
        0
    };
    let big_blind = if raw_bb > 0 {
        raw_bb
    } else if board_empty
        && sb_posted_bet > 0
        && bb_posted_bet > 0
        && (bb_posted_bet == sb_posted_bet * 2
            || (sb_posted_bet == bb_posted_bet && bb_posted_bet >= 20))
    {
        bb_posted_bet
    } else if inferred_from_table > 0 {
        inferred_from_table
    } else if hint_bb > 0 {
        hint_bb
    } else {
        BIG_BLIND
    };
    let small_blind = if raw_sb > 0 {
        raw_sb
    } else {
        (big_blind / 2).max(1)
    };
    (big_blind, small_blind)
}
