/*! WebSocket & HTTP Live Bot Client for lil-poker */
mod commit_guards;
mod fallback;
mod transport;

pub use fallback::{EmergencyFallbackRequest, build_emergency_fallback_candidates};
pub use transport::load_live_strategy;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

use crate::cfr::fallback::StrategyLookupIndex;
use crate::cfr::opponent_model::OpponentTracker;
use crate::cfr::subgame::{SubgameContext, SubgameSolver};
use crate::live::decision::{LiveDecisionRequest, decide_action_traced};
use crate::live::protocol::ActPayload;
use crate::live::state::{
    LiveRoundTracker, LiveSnapshot, LiveTableTracker, parse_live_state_with_hint,
    process_showdown_multiway,
};

use commit_guards::apply_live_commit_guards;
use fallback::choose_rejected_call_fallback;
use transport::{connect_live_ws, format_hand_cards, next_ws_stream};

struct LiveSession {
    url: String,
    room: String,
    player_id: String,
    client: reqwest::Client,
    subgame_search: bool,
    profiles_path: &'static str,
    trackers: HashMap<String, OpponentTracker>,
    current_opp_name: String,
    last_action_key: String,
    last_hole: Option<[crate::game::holdem::Card; 2]>,
    last_board_len: usize,
    hand_active: bool,
    hero_folded_this_hand: bool,
    last_seen_hand_id: u64,
    round_tracker: LiveRoundTracker,
    table_tracker: LiveTableTracker,
    subgame_solver: Arc<SubgameSolver>,
    strategy_index: Arc<StrategyLookupIndex>,
    strategy_arc: Arc<HashMap<String, Vec<f64>>>,
}

impl LiveSession {
    fn save_all_profiles(&self) {
        for (name, opp_t) in self.trackers.iter() {
            let _ = opp_t.save_profile(self.profiles_path, name);
        }
    }

    fn record_hu_opp_fold_if_unshown(&mut self, create_if_missing: bool) {
        let Some(&last_act) =
            self.round_tracker.live_history[self.round_tracker.current_round_idx].last()
        else {
            return;
        };
        if last_act < crate::game::config::RAISE_MIN || self.current_opp_name.is_empty() {
            return;
        }
        let r_num = (self.round_tracker.current_round_idx + 1) as u8;
        let opp_t = if create_if_missing {
            Some(
                self.trackers
                    .entry(self.current_opp_name.clone())
                    .or_default(),
            )
        } else {
            self.trackers.get_mut(&self.current_opp_name)
        };
        if let Some(t) = opp_t {
            t.record_action_street_detailed(crate::game::config::FOLD, r_num, r_num >= 2, 0, 0);
            if self.round_tracker.current_round_idx == 0
                && self.round_tracker.bot_3bet_preflop
                && !self.round_tracker.faced_3bet_recorded_this_hand
            {
                t.record_facing_3bet(true);
                self.round_tracker.faced_3bet_recorded_this_hand = true;
            }
        }
    }

    fn finalize_and_save_hand(&mut self) {
        self.table_tracker
            .finalize_hand(&mut self.trackers, &self.current_opp_name);
        self.save_all_profiles();
    }

    async fn handle_terminal_phase(&mut self, state: &Value, phase: &str) {
        if self.hand_active {
            if phase.eq_ignore_ascii_case("Showdown") {
                process_showdown_multiway(
                    state,
                    &self.player_id,
                    &mut self.trackers,
                    &self.current_opp_name,
                    &self.round_tracker.live_history[3],
                    self.round_tracker.last_opp_bet,
                );
            } else if self.table_tracker.last_player_count <= 2 && !self.hero_folded_this_hand {
                /* HU hand ended without Showdown after bot bet/raise -> opponent folded */
                self.record_hu_opp_fold_if_unshown(true);
            }
            self.finalize_and_save_hand();
            self.round_tracker.reset_hand(0);
            self.table_tracker.reset_hand(0);
            self.last_action_key.clear();
            self.last_hole = None;
            self.last_board_len = 0;
            self.hand_active = false;
            self.hero_folded_this_hand = false;
            self.last_seen_hand_id = 0;
        } else if phase.eq_ignore_ascii_case("Waiting") && self.last_seen_hand_id != 0 {
            /* Hand started and ended before Hero ever had an active turn (e.g. walk on BB when SB folds HU) */
            if !self.current_opp_name.is_empty()
                && let Some(players) = state.get("players").and_then(|v| v.as_array())
                && players.len() == 2
            {
                let opp_t = self
                    .trackers
                    .entry(self.current_opp_name.clone())
                    .or_default();
                opp_t.record_action_street_detailed(crate::game::config::FOLD, 1, false, 0, 0);
                opp_t.record_preflop_positional(true, false, false);
                opp_t.end_hand();
                let _ = opp_t.save_profile(self.profiles_path, &self.current_opp_name);
            }
            self.table_tracker.reset_hand(0);
            self.last_seen_hand_id = 0;
        }

        if phase.eq_ignore_ascii_case("Waiting")
            && let Some(players) = state.get("players").and_then(|v| v.as_array())
            && players.len() >= 2
        {
            let start_url = format!(
                "{}/api/game/start?room={}",
                self.url.trim_end_matches('/'),
                self.room
            );
            let _ = self.client.post(&start_url).send().await;
        }
    }

    fn ensure_current_opponent_cached(&mut self, opp_name: &str) {
        if opp_name.is_empty() {
            return;
        }
        self.current_opp_name = opp_name.to_string();
        if !self.trackers.contains_key(&self.current_opp_name) {
            if let Some(loaded) =
                OpponentTracker::load_profile(self.profiles_path, &self.current_opp_name)
            {
                println!(
                    "Loaded persistent profile for opponent '{}' ({} prior hands).",
                    self.current_opp_name, loaded.total_hands
                );
                self.trackers.insert(self.current_opp_name.clone(), loaded);
            } else {
                self.trackers
                    .insert(self.current_opp_name.clone(), OpponentTracker::new());
            }
        }
    }

    async fn decide_and_submit(&mut self, state: &Value) -> Result<(), Box<dyn std::error::Error>> {
        let phase = state.get("phase").and_then(|v| v.as_str()).unwrap_or("");
        if phase.eq_ignore_ascii_case("Showdown") || phase.eq_ignore_ascii_case("Waiting") {
            self.handle_terminal_phase(state, phase).await;
            return Ok(());
        }

        let msg_hand_id = self.update_hand_id_and_transition(state, phase);
        /* Observe every table broadcast so all 6-max opponents are tracked individually */
        let table_players = self.table_tracker.observe_state(
            state,
            &self.player_id,
            &mut self.trackers,
            Some(self.profiles_path),
        );

        let bb_hint = if self.hand_active
            && (msg_hand_id == 0 || msg_hand_id == self.round_tracker.current_hand_id)
        {
            self.round_tracker.last_big_blind
        } else {
            0
        };
        let Some(snapshot) = parse_live_state_with_hint(state, &self.player_id, bb_hint) else {
            return Ok(());
        };
        self.ensure_current_opponent_cached(&snapshot.opp_name);
        if snapshot.action_seq_key == self.last_action_key {
            return Ok(());
        }

        let tracker_clone = self.sync_snapshot_hand_and_tracker(state, &snapshot, table_players);
        let (engine_contributions, raises_this_round) = self
            .round_tracker
            .compute_scaled_subgame_contributions(&snapshot);
        let eff_chips = if snapshot.my_chips > 0 {
            snapshot.my_chips
        } else {
            self.round_tracker.estimated_my_chips
        };

        let (raw_action, raw_amount, trace) = self
            .compute_live_decision(
                &snapshot,
                tracker_clone,
                engine_contributions,
                raises_this_round,
                eff_chips,
            )
            .await?;

        let (chosen_action, amount) =
            apply_live_commit_guards(raw_action, raw_amount, &snapshot, eff_chips);
        let (hole_str, board_str) = format_hand_cards(snapshot.hole, &snapshot.board);

        println!(
            "-> Action: {:<5} (amt: {:>2}) | Hole: {:<7} | Board: {:<15} | Phase: {}",
            chosen_action.to_uppercase(),
            amount,
            hole_str,
            board_str,
            snapshot.phase
        );
        println!(
            "   [trace] round={} bucket={} pot_odds={:.1}% raw_act={} -> final_act={} reasons={:?}",
            trace.round,
            trace.bucket,
            trace.pot_odds_bps as f64 / 10.0,
            trace.raw_top_action,
            trace.final_action,
            trace.guard_reasons
        );

        self.last_action_key = snapshot.action_seq_key.clone();
        self.submit_action_with_fallback(
            &snapshot,
            &chosen_action,
            amount,
            engine_contributions,
            raises_this_round,
            eff_chips,
        )
        .await;
        Ok(())
    }

    fn update_hand_id_and_transition(&mut self, state: &Value, phase: &str) -> u64 {
        let msg_hand_id = state
            .get("hand_number")
            .or_else(|| state.get("hand_id"))
            .or_else(|| state.get("hand_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if msg_hand_id > 0 {
            self.last_seen_hand_id = msg_hand_id;
        } else if !phase.is_empty() {
            /* Track active hand even if server sends hand_id == 0 */
            self.last_seen_hand_id = self.last_seen_hand_id.max(1);
        }

        if self.hand_active
            && msg_hand_id > 0
            && msg_hand_id != self.table_tracker.hand_id
            && self.table_tracker.hand_id > 0
        {
            if self.table_tracker.last_player_count <= 2 && !self.hero_folded_this_hand {
                self.record_hu_opp_fold_if_unshown(false);
            }
            self.finalize_and_save_hand();
            self.round_tracker.reset_hand(msg_hand_id);
            self.table_tracker.reset_hand(msg_hand_id);
            self.hand_active = false;
            self.hero_folded_this_hand = false;
        } else if msg_hand_id > 0 && self.table_tracker.hand_id == 0 {
            self.table_tracker.hand_id = msg_hand_id;
        }
        msg_hand_id
    }

    fn sync_snapshot_hand_and_tracker(
        &mut self,
        state: &Value,
        snapshot: &LiveSnapshot,
        table_players: usize,
    ) -> OpponentTracker {
        let is_new_hand = snapshot.hand_id != self.round_tracker.current_hand_id
            || (self.last_hole.is_some() && self.last_hole != Some(snapshot.hole))
            || snapshot.board.len() < self.last_board_len;

        if is_new_hand {
            if self.hand_active {
                if table_players <= 2 && !self.hero_folded_this_hand {
                    self.record_hu_opp_fold_if_unshown(false);
                }
                self.finalize_and_save_hand();
                self.table_tracker.reset_hand(snapshot.hand_id);
                let _ = self.table_tracker.observe_state(
                    state,
                    &self.player_id,
                    &mut self.trackers,
                    Some(self.profiles_path),
                );
            }
            self.round_tracker.reset_hand(snapshot.hand_id);
            self.hero_folded_this_hand = false;
        }
        self.last_hole = Some(snapshot.hole);
        self.last_board_len = snapshot.board.len();
        self.hand_active = true;

        let active_tracker = self
            .trackers
            .entry(self.current_opp_name.clone())
            .or_default();
        active_tracker.observe_blind_level(snapshot.big_blind);
        if table_players > 2 {
            /* In 6-max, LiveTableTracker records each opponent's exact actions individually;
             * keep round_tracker synced for live_history without cross-contaminating trackers. */
            self.round_tracker
                .update_street_with_tracker(snapshot.board.len(), None);
            self.round_tracker.infer_opponent_history_only(snapshot);
        } else {
            self.round_tracker
                .update_street_with_tracker(snapshot.board.len(), Some(active_tracker));
            self.round_tracker
                .infer_and_record_opponent_action(snapshot, active_tracker);
        }
        active_tracker.clone()
    }

    async fn compute_live_decision(
        &self,
        snapshot: &LiveSnapshot,
        tracker_clone: OpponentTracker,
        engine_contributions: [i32; 2],
        raises_this_round: u8,
        eff_chips: i32,
    ) -> Result<(String, i32, crate::cfr::opponent_model::DecisionTrace), tokio::task::JoinError>
    {
        let strat_clone = Arc::clone(&self.strategy_arc);
        let index_clone = Arc::clone(&self.strategy_index);
        let solver_arc = Arc::clone(&self.subgame_solver);
        let subgame = self.subgame_search;
        let board_cards_clone = snapshot.board.clone();
        let history_clone = self.round_tracker.live_history;
        let hole = snapshot.hole;
        let legal_actions = snapshot.legal_actions.clone();
        let to_call = snapshot.to_call;
        let pot = snapshot.pot;
        let my_player = snapshot.my_player;
        let active_opponents = snapshot.active_opponents;
        let big_blind = snapshot.big_blind;
        let opp_chips = snapshot.opp_chips;
        let current_bet_now = snapshot.current_bet;

        tokio::task::spawn_blocking(move || {
            let mut ctx = SubgameContext::new();
            decide_action_traced(
                &LiveDecisionRequest {
                    hole,
                    board: &board_cards_clone,
                    legal: &legal_actions,
                    to_call,
                    pot,
                    subgame_search: subgame,
                    history: history_clone,
                    my_player,
                    contributions: engine_contributions,
                    raises_this_round,
                    active_opponents,
                    big_blind,
                    my_chips: eff_chips,
                    opp_chips,
                    current_bet: current_bet_now,
                    strategy_index: Some(&index_clone),
                    solver: Some(&solver_arc),
                },
                &strat_clone,
                &tracker_clone,
                Some(&mut ctx),
            )
        })
        .await
    }

    async fn send_act_request(
        &self,
        act_url: &str,
        action: &str,
        amount: i32,
    ) -> Result<reqwest::Response, reqwest::Error> {
        self.client
            .post(act_url)
            .json(&ActPayload {
                player_id: self.player_id.clone(),
                action: action.to_string(),
                amount,
            })
            .send()
            .await
    }

    async fn submit_action_with_fallback(
        &mut self,
        snapshot: &LiveSnapshot,
        chosen_action: &str,
        amount: i32,
        engine_contributions: [i32; 2],
        raises_this_round: u8,
        my_chips: i32,
    ) {
        let act_url = format!(
            "{}/api/game/act?room={}",
            self.url.trim_end_matches('/'),
            self.room
        );
        let resp = self.send_act_request(&act_url, chosen_action, amount).await;

        let Ok(r) = resp else {
            if let Err(e) = resp {
                eprintln!("Act HTTP network error: {e}");
                self.last_action_key.clear();
            }
            return;
        };

        if r.status().is_success() {
            self.record_accepted_bot_action(chosen_action, amount, snapshot);
            return;
        }

        let status = r.status();
        let err_text = r.text().await.unwrap_or_default();
        eprintln!("⚠️ Act server notice ({chosen_action}) [{status}]: {err_text}");
        self.last_action_key.clear();

        let first_retry = self.choose_rejected_action_fallback(
            snapshot,
            chosen_action,
            &err_text,
            engine_contributions,
            raises_this_round,
            my_chips,
        );
        if let Some((ref fb, fb_amt)) = first_retry {
            println!(
                "-> Auto-fallback: Retrying action {} (amt: {})",
                fb.to_uppercase(),
                fb_amt
            );
            if let Ok(r_retry) = self.send_act_request(&act_url, fb, fb_amt).await
                && r_retry.status().is_success()
            {
                self.record_accepted_bot_action(fb, fb_amt, snapshot);
                return;
            }
        }

        /* Emergency pot-commit / all-in call rescue: try safe non-escalating options before all-in */
        let is_priced_in = snapshot.to_call > 0
            && (snapshot.to_call <= snapshot.big_blind.max(1) / 2
                || snapshot.to_call * 8 <= (snapshot.pot + snapshot.to_call).max(1)
                || (my_chips > 0 && my_chips <= snapshot.to_call));
        if is_priced_in || chosen_action == "call" {
            let candidates = build_emergency_fallback_candidates(&EmergencyFallbackRequest {
                legal_actions: &snapshot.legal_actions,
                chosen_action,
                chosen_amount: amount,
                first_retry: first_retry.as_ref().map(|(a, amt)| (a.as_str(), *amt)),
                err_text: &err_text,
                to_call: snapshot.to_call,
                my_chips,
            });
            for (cand, cand_amt) in candidates {
                if let Ok(r_em) = self.send_act_request(&act_url, cand, cand_amt).await
                    && r_em.status().is_success()
                {
                    self.record_accepted_bot_action(cand, cand_amt, snapshot);
                    return;
                }
            }
        }
    }

    fn record_accepted_bot_action(&mut self, action: &str, amount: i32, snapshot: &LiveSnapshot) {
        /* Record bot's own action into live history only after server accepts it */
        self.round_tracker.record_bot_action_with_bet(
            action,
            amount,
            snapshot.pot,
            snapshot.my_bet,
        );
        if action == "fold" && self.hand_active {
            self.hero_folded_this_hand = true;
        }
    }

    fn choose_rejected_action_fallback(
        &self,
        snapshot: &LiveSnapshot,
        chosen_action: &str,
        err_text: &str,
        engine_contributions: [i32; 2],
        raises_this_round: u8,
        my_chips: i32,
    ) -> Option<(String, i32)> {
        let has_legal = |tok: &str| {
            snapshot
                .legal_actions
                .iter()
                .any(|a| a.eq_ignore_ascii_case(tok))
        };
        if chosen_action == "check"
            && (err_text.contains("cannot check") || err_text.contains("call"))
        {
            Some(("call".to_string(), 0))
        } else if chosen_action == "call" {
            choose_rejected_call_fallback(
                &snapshot.legal_actions,
                err_text,
                snapshot.to_call,
                my_chips,
            )
        } else if chosen_action == "all_in" || chosen_action == "allin" {
            if snapshot.to_call > 0 && snapshot.to_call >= my_chips && has_legal("call") {
                Some(("call".to_string(), 0))
            } else if my_chips > 0 && has_legal("raise") {
                Some(("raise".to_string(), my_chips + snapshot.my_bet))
            } else if my_chips > 0 && has_legal("bet") {
                Some(("bet".to_string(), my_chips + snapshot.my_bet))
            } else if snapshot.to_call > 0 && has_legal("call") {
                Some(("call".to_string(), 0))
            } else {
                None
            }
        } else if chosen_action == "raise" || chosen_action == "bet" {
            if snapshot.to_call == 0 {
                return Some(("check".to_string(), 0));
            }
            self.choose_non_raise_fallback(
                snapshot,
                engine_contributions,
                raises_this_round,
                my_chips,
            )
        } else {
            None
        }
    }

    fn choose_non_raise_fallback(
        &self,
        snapshot: &LiveSnapshot,
        engine_contributions: [i32; 2],
        raises_this_round: u8,
        my_chips: i32,
    ) -> Option<(String, i32)> {
        let non_raise_legal: Vec<String> = snapshot
            .legal_actions
            .iter()
            .filter(|a| {
                a.eq_ignore_ascii_case("fold")
                    || a.eq_ignore_ascii_case("call")
                    || a.eq_ignore_ascii_case("check")
            })
            .cloned()
            .collect();
        if non_raise_legal.is_empty() {
            return None;
        }
        let fb_tracker = self
            .trackers
            .get(&self.current_opp_name)
            .cloned()
            .unwrap_or_default();
        let (fb_act, fb_amt, _) = decide_action_traced(
            &LiveDecisionRequest {
                hole: snapshot.hole,
                board: &snapshot.board,
                legal: &non_raise_legal,
                to_call: snapshot.to_call,
                pot: snapshot.pot,
                subgame_search: false,
                history: self.round_tracker.live_history,
                my_player: snapshot.my_player,
                contributions: engine_contributions,
                raises_this_round,
                active_opponents: snapshot.active_opponents,
                big_blind: snapshot.big_blind,
                my_chips,
                opp_chips: snapshot.opp_chips,
                current_bet: snapshot.current_bet,
                strategy_index: Some(&self.strategy_index),
                solver: None,
            },
            &self.strategy_arc,
            &fb_tracker,
            None,
        );
        Some((fb_act, fb_amt))
    }
}

/** Connect to the live game server and run the continuous bot decision loop */
pub async fn run_live_bot(
    url: &str,
    room: &str,
    name: &str,
    strategy_path: &str,
    subgame_search: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== lil-poker-mccfr Live Rust Bot Client ===");
    println!("Server URL: {url}");
    println!("Room ID:    {room}");
    println!("Bot Name:   {name}");

    /* 1. Load Strategy Model (Target: <= 400MB RAM) */
    let strategy_map = load_live_strategy(strategy_path);
    let ram_mb = (strategy_map.len() * 140) as f64 / (1024.0 * 1024.0);
    println!(
        "Loaded strategy with {} infosets (RAM footprint: ~{:.1} MB).",
        strategy_map.len(),
        ram_mb
    );

    /* 2-4. Login, join room & connect initial WebSocket */
    let Some((jar, client, player_id, initial_ws)) = connect_live_ws(url, room, name).await? else {
        return Ok(());
    };

    let strategy_index = Arc::new(StrategyLookupIndex::build_from_map(&strategy_map));
    let mut session = LiveSession {
        url: url.to_string(),
        room: room.to_string(),
        player_id,
        client,
        subgame_search,
        profiles_path: "models/opponent_profiles.json",
        trackers: HashMap::new(),
        current_opp_name: String::new(),
        last_action_key: String::new(),
        last_hole: None,
        last_board_len: 0,
        hand_active: false,
        hero_folded_this_hand: false,
        last_seen_hand_id: 0,
        round_tracker: LiveRoundTracker::new(),
        table_tracker: LiveTableTracker::new(),
        subgame_solver: Arc::new(SubgameSolver::default()),
        strategy_index,
        strategy_arc: Arc::new(strategy_map),
    };

    /* 5. Reconnect-resilient WebSocket session loop preserving cookie jar, player_id, and active hand state */
    let mut current_ws = Some(initial_ws);
    let mut backoff_secs = 2u64;
    loop {
        let ws_stream = next_ws_stream(url, room, &jar, &mut current_ws, &mut backoff_secs).await;
        let (mut write, mut read) = ws_stream.split();

        while let Some(msg) = read.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("WebSocket error: {e}");
                    break;
                }
            };
            match msg {
                Message::Ping(payload) => {
                    let _ = write.send(Message::Pong(payload)).await;
                }
                Message::Close(_) => {
                    println!("WebSocket closed by server.");
                    break;
                }
                Message::Text(text) => {
                    let Ok(state) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    if let Err(e) = session.decide_and_submit(&state).await {
                        eprintln!("Decision/submit error: {e}");
                    }
                }
                _ => {}
            }
        }

        session.save_all_profiles();
        eprintln!("WebSocket disconnected. Reconnecting in {backoff_secs}s...");
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(30);
    }
}
