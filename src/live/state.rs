/* Live Poker State Snapshot and Round History Tracker */
use std::collections::HashMap;

use serde_json::Value;

use super::protocol::{parse_card_str, parse_json_i32};
use crate::cfr::opponent_model::OpponentTracker;
use crate::game::config::*;
use crate::game::holdem::{
    Card, RoundHistory, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};

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
    let mut sb_posted_bet = 0i32;
    let mut bb_posted_bet = 0i32;
    let mut opp_name = String::new();
    let mut active_opps = 0usize;
    let mut total_opps = 0usize;
    let mut best_opp_score = i64::MIN;
    let mut opp_bets = Vec::new();

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
            if is_sb && p_bet > 0 {
                sb_posted_bet = p_bet;
            }
            if is_bb && p_bet > 0 {
                bb_posted_bet = p_bet;
            }
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
                opp_bets.push(p_bet);
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
     * and subtract uncallable overbet chips across ALL opponents from pot without wiping prior pot. */
    let to_call = if my_chips > 0 {
        raw_to_call.min(my_chips)
    } else {
        raw_to_call
    };
    let raw_pot = parse_json_i32(state.get("pot"));
    let single_uncallable = raw_to_call.saturating_sub(to_call);
    let total_uncallable = if my_chips > 0 {
        let my_max_street_bet = my_bet + my_chips;
        let multi_uncallable: i32 = opp_bets
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
        let bb = parse_json_i32(
            state
                .get("big_blind")
                .or_else(|| state.get("bigBlind"))
                .or_else(|| state.get("bb"))
                .or_else(|| state.get("min_bet"))
                .or_else(|| state.get("minBet")),
        );
        if bb > 0 {
            bb
        } else if board.is_empty() && sb_posted_bet > 0 && bb_posted_bet == sb_posted_bet * 2 {
            bb_posted_bet
        } else {
            BIG_BLIND
        }
    };
    let small_blind = {
        let sb = parse_json_i32(
            state
                .get("small_blind")
                .or_else(|| state.get("smallBlind"))
                .or_else(|| state.get("sb")),
        );
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

/* Process showdown event: inspect each opponent's cards and record into their own OpponentTracker */
pub fn process_showdown_multiway(
    state: &Value,
    player_id: &str,
    trackers: &mut HashMap<String, OpponentTracker>,
    fallback_opp_name: &str,
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
                    let p_name = p
                        .get("name")
                        .or_else(|| p.get("username"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(if !p_id.is_empty() {
                            p_id
                        } else {
                            fallback_opp_name
                        });
                    if !p_name.is_empty() {
                        let tracker = trackers.entry(p_name.to_string()).or_default();
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

/* In-place per-opponent multiway & 6-max hand state tracking */
#[derive(Debug, Clone, Default)]
pub struct OppHandState {
    pub last_bet: i32,
    pub is_folded: bool,
    pub is_allin: bool,
    pub is_sb: bool,
    pub acted_pf: bool,
    pub pf_vpip: bool,
    pub pf_pfr: bool,
    pub pf_raises_made: u8,
    pub faced_3bet_recorded: bool,
    pub initialized: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LiveTableTracker {
    pub hand_id: u64,
    pub round_idx: usize,
    pub street_max_bet: i32,
    pub pf_raise_count: u8,
    pub my_last_bet: i32,
    pub last_player_count: usize,
    pub opp_states: HashMap<String, OppHandState>,
    pub big_blind: i32,
    pub small_blind: i32,
}

fn resolve_player_key(p: &Value, p_id: &str) -> String {
    p.get("name")
        .or_else(|| p.get("username"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(p_id)
        .to_string()
}

fn is_player_folded(p: &Value) -> bool {
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

fn classify_live_bet_action(delta: i32, pot: i32, bb: i32, is_allin: bool) -> u8 {
    if is_allin || delta >= (bb * 35).max(1) {
        ALL_IN
    } else if delta >= (pot / 2).max(bb) {
        RAISE_HALF_POT
    } else if delta >= (pot / 3).max(bb) {
        RAISE_THIRD_POT
    } else {
        RAISE_MIN
    }
}

impl LiveTableTracker {
    pub fn new() -> Self {
        Self {
            big_blind: BIG_BLIND,
            small_blind: SMALL_BLIND,
            ..Default::default()
        }
    }

    pub fn reset_hand(&mut self, hand_id: u64) {
        self.hand_id = hand_id;
        self.round_idx = 0;
        self.street_max_bet = 0;
        self.pf_raise_count = 0;
        self.my_last_bet = 0;
        self.last_player_count = 0;
        self.opp_states.clear();
    }

    /* Observe any WebSocket table update (even when not Hero's turn) so every 6-max
     * opponent's actions, folds, 3-bet responses, and hand participation are tracked individually. */
    pub fn observe_state(
        &mut self,
        state: &Value,
        my_player_id: &str,
        trackers: &mut HashMap<String, OpponentTracker>,
        profiles_path: Option<&str>,
    ) -> usize {
        let Some(players) = state.get("players").and_then(|v| v.as_array()) else {
            return 0;
        };
        let player_count = players.len();
        self.last_player_count = player_count;

        let board_len = state
            .get("board")
            .and_then(|v| v.as_array())
            .map(|b| b.len())
            .unwrap_or(0);
        let new_round_idx = match board_len {
            0 => 0,
            3 => 1,
            4 => 2,
            _ => 3,
        };
        let pot = parse_json_i32(state.get("pot"));
        let parsed_bb = parse_json_i32(
            state
                .get("big_blind")
                .or_else(|| state.get("bigBlind"))
                .or_else(|| state.get("bb"))
                .or_else(|| state.get("min_bet"))
                .or_else(|| state.get("minBet")),
        );
        if parsed_bb > 0 {
            self.big_blind = parsed_bb;
        }
        let parsed_sb = parse_json_i32(
            state
                .get("small_blind")
                .or_else(|| state.get("smallBlind"))
                .or_else(|| state.get("sb")),
        );
        self.small_blind = if parsed_sb > 0 {
            parsed_sb
        } else {
            (self.big_blind / 2).max(1)
        };
        let bb = self.big_blind.max(1);
        let sb = self.small_blind.max(1);

        if new_round_idx > self.round_idx {
            let prev_round_num = (self.round_idx + 1) as u8;
            /* Resolve preflop folds or calls that occurred right as the server dealt the next street */
            for p in players {
                let p_id = p
                    .get("id")
                    .or_else(|| p.get("uuid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if p_id.eq_ignore_ascii_case(my_player_id) {
                    continue;
                }
                let opp_key = resolve_player_key(p, p_id);
                if opp_key.is_empty() {
                    continue;
                }
                let Some(st) = self.opp_states.get_mut(&opp_key) else {
                    continue;
                };
                if st.is_folded {
                    continue;
                }
                let now_folded = is_player_folded(p);
                let tracker = trackers.entry(opp_key).or_default();
                if now_folded {
                    let facing_bet = self.street_max_bet > st.last_bet;
                    tracker.record_action_street_detailed(FOLD, prev_round_num, facing_bet, 0, pot);
                    if self.round_idx == 0 {
                        st.acted_pf = true;
                        if st.pf_pfr && self.pf_raise_count >= 2 && !st.faced_3bet_recorded {
                            tracker.record_facing_3bet(true);
                            st.faced_3bet_recorded = true;
                        }
                    }
                    st.is_folded = true;
                } else if self.round_idx == 0 && !st.is_allin && st.last_bet < self.street_max_bet {
                    let delta = self.street_max_bet - st.last_bet;
                    st.acted_pf = true;
                    st.pf_vpip = true;
                    if st.pf_pfr && self.pf_raise_count >= 2 && !st.faced_3bet_recorded {
                        tracker.record_facing_3bet(false);
                        st.faced_3bet_recorded = true;
                    }
                    tracker.record_action_street_detailed(CALL_CHECK, 1, true, delta, pot);
                }
            }
            if player_count > 2 && self.round_idx >= 1 && self.street_max_bet == 0 {
                for (name, st) in &self.opp_states {
                    if !st.is_folded && !st.is_allin {
                        trackers
                            .entry(name.clone())
                            .or_default()
                            .record_action_street_detailed(
                                CALL_CHECK,
                                prev_round_num,
                                false,
                                0,
                                pot,
                            );
                    }
                }
            }
            for st in self.opp_states.values_mut() {
                st.last_bet = 0;
            }
            self.my_last_bet = 0;
            self.street_max_bet = 0;
            self.round_idx = new_round_idx;
        }

        if player_count <= 2 {
            for p in players {
                let p_id = p
                    .get("id")
                    .or_else(|| p.get("uuid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if p_id.eq_ignore_ascii_case(my_player_id) {
                    continue;
                }
                let opp_key = resolve_player_key(p, p_id);
                if !opp_key.is_empty() {
                    let is_sb = p
                        .get("is_small_blind")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    self.opp_states
                        .entry(opp_key)
                        .or_insert_with(|| OppHandState {
                            is_sb,
                            initialized: true,
                            ..Default::default()
                        });
                }
            }
            return player_count;
        }

        /* First sync Hero's own bet so street_max_bet and pf_raise_count stay accurate */
        for p in players {
            let p_id = p
                .get("id")
                .or_else(|| p.get("uuid"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !p_id.eq_ignore_ascii_case(my_player_id) {
                continue;
            }
            let my_bet = parse_json_i32(p.get("bet").or_else(|| p.get("current_bet")));
            let is_sb = p
                .get("is_small_blind")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let is_bb = p
                .get("is_big_blind")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if self.round_idx == 0
                && self.my_last_bet == 0
                && ((is_sb && my_bet <= sb) || (is_bb && my_bet <= bb))
            {
                self.my_last_bet = my_bet;
                self.street_max_bet = self.street_max_bet.max(my_bet);
            }
            if my_bet > self.my_last_bet {
                if my_bet > self.street_max_bet {
                    if self.round_idx == 0 {
                        self.pf_raise_count = self.pf_raise_count.saturating_add(1);
                    }
                    self.street_max_bet = my_bet;
                }
                self.my_last_bet = my_bet;
            }
        }

        /* Record each individual 6-max opponent into their own OpponentTracker */
        for p in players {
            let p_id = p
                .get("id")
                .or_else(|| p.get("uuid"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if p_id.eq_ignore_ascii_case(my_player_id) {
                continue;
            }
            let opp_key = resolve_player_key(p, p_id);
            if opp_key.is_empty() {
                continue;
            }
            if !trackers.contains_key(&opp_key) {
                if let Some(path) = profiles_path {
                    if let Some(loaded) = OpponentTracker::load_profile(path, &opp_key) {
                        trackers.insert(opp_key.clone(), loaded);
                    } else {
                        trackers.insert(opp_key.clone(), OpponentTracker::new());
                    }
                } else {
                    trackers.insert(opp_key.clone(), OpponentTracker::new());
                }
            }
            let tracker = trackers.entry(opp_key.clone()).or_default();
            tracker.observe_blind_level(bb);

            let p_bet = parse_json_i32(p.get("bet").or_else(|| p.get("current_bet")));
            let p_chips = parse_json_i32(
                p.get("chips")
                    .or_else(|| p.get("stack"))
                    .or_else(|| p.get("balance"))
                    .or_else(|| p.get("remaining_chips"))
                    .or_else(|| p.get("chip_count")),
            );
            let is_sb = p
                .get("is_small_blind")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let is_bb = p
                .get("is_big_blind")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let now_folded = is_player_folded(p);
            let now_allin = p_chips == 0 && p_bet > 0;

            let st = self
                .opp_states
                .entry(opp_key)
                .or_insert_with(|| OppHandState {
                    is_sb,
                    ..Default::default()
                });
            st.is_sb = is_sb;

            if !st.initialized {
                if self.round_idx == 0 {
                    if (is_sb && p_bet <= sb) || (is_bb && p_bet <= bb) {
                        st.last_bet = p_bet;
                        self.street_max_bet = self.street_max_bet.max(bb);
                    } else if p_bet > 0 {
                        st.last_bet = if is_sb {
                            sb.min(p_bet)
                        } else if is_bb {
                            bb.min(p_bet)
                        } else {
                            0
                        };
                        self.street_max_bet = self.street_max_bet.max(bb);
                    }
                }
                st.initialized = true;
            }

            if !st.is_folded && now_folded {
                let facing_bet = self.street_max_bet > st.last_bet;
                tracker.record_action_street_detailed(
                    FOLD,
                    (self.round_idx + 1) as u8,
                    facing_bet,
                    0,
                    pot,
                );
                if self.round_idx == 0 {
                    st.acted_pf = true;
                    if st.pf_pfr && self.pf_raise_count >= 2 && !st.faced_3bet_recorded {
                        tracker.record_facing_3bet(true);
                        st.faced_3bet_recorded = true;
                    }
                }
                st.is_folded = true;
            } else if p_bet > st.last_bet {
                let delta = p_bet - st.last_bet;
                let round_num = (self.round_idx + 1) as u8;
                let facing_bet = self.street_max_bet > st.last_bet;
                if p_bet > self.street_max_bet {
                    if self.round_idx == 0 {
                        self.pf_raise_count = self.pf_raise_count.saturating_add(1);
                        if st.pf_pfr && self.pf_raise_count >= 2 && !st.faced_3bet_recorded {
                            tracker.record_facing_3bet(false);
                            st.faced_3bet_recorded = true;
                        }
                        st.acted_pf = true;
                        st.pf_vpip = true;
                        st.pf_pfr = true;
                        st.pf_raises_made = st.pf_raises_made.saturating_add(1);
                    }
                    let act = classify_live_bet_action(delta, pot, bb, now_allin);
                    tracker.record_action_street_detailed(act, round_num, facing_bet, delta, pot);
                    self.street_max_bet = p_bet;
                } else {
                    if self.round_idx == 0 {
                        st.acted_pf = true;
                        st.pf_vpip = true;
                        if st.pf_pfr && self.pf_raise_count >= 2 && !st.faced_3bet_recorded {
                            tracker.record_facing_3bet(false);
                            st.faced_3bet_recorded = true;
                        }
                    }
                    tracker.record_action_street_detailed(
                        CALL_CHECK, round_num, facing_bet, delta, pot,
                    );
                }
                st.last_bet = p_bet;
            }
            st.is_allin = now_allin;
        }

        player_count
    }

    pub fn finalize_hand(
        &mut self,
        trackers: &mut HashMap<String, OpponentTracker>,
        fallback_opp_name: &str,
    ) {
        let multiway_positional = self.last_player_count > 2;
        let mut finalized = std::collections::HashSet::with_capacity(self.opp_states.len() + 1);
        for (name, st) in self.opp_states.drain() {
            if let Some(t) = trackers.get_mut(&name) {
                if multiway_positional && st.acted_pf {
                    t.record_preflop_positional(st.is_sb, st.pf_vpip, st.pf_pfr);
                }
                t.end_hand();
            }
            finalized.insert(name);
        }
        if !fallback_opp_name.is_empty() && !finalized.contains(fallback_opp_name) {
            if let Some(t) = trackers.get_mut(fallback_opp_name) {
                t.end_hand();
            }
        }
        self.round_idx = 0;
        self.street_max_bet = 0;
        self.pf_raise_count = 0;
        self.my_last_bet = 0;
        self.last_player_count = 0;
    }
}

pub use super::round_tracker::LiveRoundTracker;
