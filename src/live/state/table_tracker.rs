use std::collections::HashMap;

use serde_json::Value;

use super::{
    extract_blind_flags, extract_player_bet, extract_player_chips, extract_player_id,
    extract_player_name, is_player_folded, parse_cards_array, resolve_blinds,
};
use crate::cfr::opponent_model::OpponentTracker;
use crate::game::config::{BIG_BLIND, SMALL_BLIND};
use crate::game::holdem::{
    RoundHistory, ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use crate::live::protocol::parse_json_i32;

/* Process showdown event: inspect each opponent's cards and record into their own OpponentTracker */
pub fn process_showdown_multiway(
    state: &Value,
    player_id: &str,
    trackers: &mut HashMap<String, OpponentTracker>,
    fallback_opp_name: &str,
    river_history: &RoundHistory,
    last_opp_bet: i32,
) {
    let Some(players) = state.get("players").and_then(|v| v.as_array()) else {
        return;
    };
    let b_cards = parse_cards_array(state.get("board"));
    if b_cards.len() < 3 {
        return;
    }
    let is_hu = players.len() <= 2;
    let hu_opp_raised_river =
        last_opp_bet > 0 || (river_history.len() == 1 && river_history[0] >= RAISE_MIN);
    let pot_showdown = parse_json_i32(state.get("pot"));

    for p in players {
        let p_id = extract_player_id(p);
        if p_id.eq_ignore_ascii_case(player_id) {
            continue;
        }
        let opp_hole = parse_cards_array(p.get("hole"));
        if opp_hole.len() != 2 {
            continue;
        }
        let fallback_id = if !p_id.is_empty() {
            p_id
        } else {
            fallback_opp_name
        };
        let p_name = extract_player_name(p, fallback_id);
        if !p_name.is_empty() {
            let p_bet = extract_player_bet(p);
            let (p_raised_river, p_last_bet) = if is_hu {
                (hu_opp_raised_river, last_opp_bet.max(p_bet))
            } else {
                (p_bet > 0, p_bet)
            };
            let tracker = trackers.entry(p_name.to_string()).or_default();
            tracker.record_showdown_hand_detailed(
                [opp_hole[0], opp_hole[1]],
                &b_cards,
                p_raised_river,
                p_last_bet,
                pot_showdown,
            );
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
    let Some(players) = state.get("players").and_then(|v| v.as_array()) else {
        return;
    };
    let b_cards = parse_cards_array(state.get("board"));
    if b_cards.len() < 3 {
        return;
    }
    let opp_raised_river =
        last_opp_bet > 0 || (river_history.len() == 1 && river_history[0] >= RAISE_MIN);
    let pot_showdown = parse_json_i32(state.get("pot"));

    for p in players {
        if extract_player_id(p).eq_ignore_ascii_case(player_id) {
            continue;
        }
        let opp_hole = parse_cards_array(p.get("hole"));
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

fn record_facing_3bet_once(
    tracker: &mut OpponentTracker,
    st: &mut OppHandState,
    round_idx: usize,
    pf_raise_count: u8,
    folded: bool,
) {
    if round_idx == 0 && st.pf_pfr && pf_raise_count >= 2 && !st.faced_3bet_recorded {
        tracker.record_facing_3bet(folded);
        st.faced_3bet_recorded = true;
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

    fn advance_street(
        &mut self,
        players: &[Value],
        my_player_id: &str,
        trackers: &mut HashMap<String, OpponentTracker>,
        new_round_idx: usize,
        pot: i32,
    ) {
        let prev_round_num = (self.round_idx + 1) as u8;
        /* Resolve preflop folds or calls that occurred right as the server dealt the next street */
        for p in players {
            let p_id = extract_player_id(p);
            if p_id.eq_ignore_ascii_case(my_player_id) {
                continue;
            }
            let opp_key = extract_player_name(p, p_id).to_string();
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
                }
                record_facing_3bet_once(tracker, st, self.round_idx, self.pf_raise_count, true);
                st.is_folded = true;
            } else if !st.is_allin && st.last_bet < self.street_max_bet {
                let delta = self.street_max_bet - st.last_bet;
                if self.round_idx == 0 {
                    st.acted_pf = true;
                    st.pf_vpip = true;
                    record_facing_3bet_once(
                        tracker,
                        st,
                        self.round_idx,
                        self.pf_raise_count,
                        false,
                    );
                }
                tracker.record_action_street_detailed(CALL_CHECK, prev_round_num, true, delta, pot);
            }
        }
        if players.len() > 2 && self.round_idx >= 1 && self.street_max_bet == 0 {
            for (name, st) in &self.opp_states {
                if !st.is_folded && !st.is_allin {
                    trackers
                        .entry(name.clone())
                        .or_default()
                        .record_action_street_detailed(CALL_CHECK, prev_round_num, false, 0, pot);
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

    fn sync_hero_bet(&mut self, players: &[Value], my_player_id: &str, bb: i32, sb: i32) {
        for p in players {
            if !extract_player_id(p).eq_ignore_ascii_case(my_player_id) {
                continue;
            }
            let my_bet = extract_player_bet(p);
            let (is_sb, is_bb) = extract_blind_flags(p);
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
    }

    fn observe_multiway_opponents(
        &mut self,
        players: &[Value],
        my_player_id: &str,
        trackers: &mut HashMap<String, OpponentTracker>,
        profiles_path: Option<&str>,
        pot: i32,
    ) {
        let bb = self.big_blind.max(1);
        let sb = self.small_blind.max(1);
        for p in players {
            let p_id = extract_player_id(p);
            if p_id.eq_ignore_ascii_case(my_player_id) {
                continue;
            }
            let opp_key = extract_player_name(p, p_id).to_string();
            if opp_key.is_empty() {
                continue;
            }
            if !trackers.contains_key(&opp_key) {
                let loaded =
                    profiles_path.and_then(|path| OpponentTracker::load_profile(path, &opp_key));
                trackers.insert(opp_key.clone(), loaded.unwrap_or_default());
            }
            let tracker = trackers.entry(opp_key.clone()).or_default();
            tracker.observe_blind_level(bb);

            let p_bet = extract_player_bet(p);
            let p_chips = extract_player_chips(p);
            let (is_sb, is_bb) = extract_blind_flags(p);
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
                }
                record_facing_3bet_once(tracker, st, self.round_idx, self.pf_raise_count, true);
                st.is_folded = true;
            } else if p_bet > st.last_bet {
                let delta = p_bet - st.last_bet;
                let round_num = (self.round_idx + 1) as u8;
                let facing_bet = self.street_max_bet > st.last_bet;
                if p_bet > self.street_max_bet {
                    if self.round_idx == 0 {
                        self.pf_raise_count = self.pf_raise_count.saturating_add(1);
                        record_facing_3bet_once(
                            tracker,
                            st,
                            self.round_idx,
                            self.pf_raise_count,
                            false,
                        );
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
                        record_facing_3bet_once(
                            tracker,
                            st,
                            self.round_idx,
                            self.pf_raise_count,
                            false,
                        );
                    }
                    tracker.record_action_street_detailed(
                        CALL_CHECK, round_num, facing_bet, delta, pot,
                    );
                }
                st.last_bet = p_bet;
            }
            st.is_allin = now_allin;
        }
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
        let mut sb_posted = 0i32;
        let mut bb_posted = 0i32;
        let mut nonzero_bets = Vec::new();
        for p in players {
            let bet = extract_player_bet(p);
            if bet > 0 {
                nonzero_bets.push(bet);
            }
            let (is_sb, is_bb) = extract_blind_flags(p);
            if is_sb && bet > 0 {
                sb_posted = bet;
            }
            if is_bb && bet > 0 {
                bb_posted = bet;
            }
        }
        let (resolved_bb, resolved_sb) = resolve_blinds(
            state,
            board_len == 0,
            sb_posted,
            bb_posted,
            &nonzero_bets,
            pot,
            self.big_blind,
        );
        if resolved_bb > 0 {
            self.big_blind = resolved_bb;
        }
        self.small_blind = if resolved_sb > 0 {
            resolved_sb
        } else {
            (self.big_blind / 2).max(1)
        };
        let bb = self.big_blind.max(1);
        let sb = self.small_blind.max(1);

        if new_round_idx > self.round_idx {
            self.advance_street(players, my_player_id, trackers, new_round_idx, pot);
        }

        if player_count <= 2 {
            for p in players {
                let p_id = extract_player_id(p);
                if p_id.eq_ignore_ascii_case(my_player_id) {
                    continue;
                }
                let opp_key = extract_player_name(p, p_id).to_string();
                if !opp_key.is_empty() {
                    let (is_sb, _) = extract_blind_flags(p);
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
        self.sync_hero_bet(players, my_player_id, bb, sb);
        /* Record each individual 6-max opponent into their own OpponentTracker */
        self.observe_multiway_opponents(players, my_player_id, trackers, profiles_path, pot);

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
