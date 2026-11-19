/* WebSocket & HTTP Live Bot Client for lil-poker */
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reqwest::cookie::CookieStore;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use super::decision::decide_action_traced_with_bet;
use super::protocol::{ActPayload, GuestResponse, JoinRoomPayload};
use super::state::{
    parse_live_state_with_hint, process_showdown_multiway, LiveRoundTracker, LiveSnapshot,
    LiveTableTracker,
};
use crate::cfr::fallback::StrategyLookupIndex;
use crate::cfr::opponent_model::OpponentTracker;
use crate::cfr::subgame::{SubgameContext, SubgameSolver};

pub fn load_live_strategy(path: &str) -> HashMap<String, Vec<f64>> {
    if std::path::Path::new(path).exists() {
        println!("Loading strategy model from {}...", path);
        if let Ok(file) = File::open(path) {
            let mut reader = BufReader::new(file);
            if path.ends_with(".bin") || path.ends_with(".bincode") {
                if let Ok(raw_f32) = bincode::deserialize_from::<
                    _,
                    HashMap<String, [f32; crate::game::config::HOLDEM_NUM_ACTIONS]>,
                >(&mut reader)
                {
                    let mut map = HashMap::new();
                    for (k, v) in raw_f32 {
                        map.insert(k, v.iter().map(|&x| x as f64).collect());
                    }
                    return map;
                }
                if let Ok(file_retry) = File::open(path) {
                    let reader_retry = BufReader::new(file_retry);
                    if let Ok(raw) =
                        bincode::deserialize_from::<_, HashMap<String, Vec<f64>>>(reader_retry)
                    {
                        return raw;
                    }
                }
            } else if let Ok(raw) = serde_json::from_reader::<_, HashMap<String, Vec<f64>>>(reader)
            {
                let mut map = HashMap::new();
                for (k, v) in raw {
                    if v.len() >= 6 {
                        map.insert(k, v);
                    } else if v.len() == 4 {
                        /* Map legacy 4-action entries [fold, call, raise_min, raise_half_pot] */
                        map.insert(k, vec![v[0], v[1], v[2], 0.0, v[3], 0.0]);
                    } else {
                        map.insert(k, v);
                    }
                }
                return map;
            }
        }
    }
    println!(
        "WARNING: Strategy file {} not found! Using random play.",
        path
    );
    HashMap::new()
}

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

async fn open_ws_stream(
    url: &str,
    room: &str,
    jar: &reqwest::cookie::Jar,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Box<dyn std::error::Error>> {
    let ws_scheme = if url.starts_with("https://") {
        "wss"
    } else {
        "ws"
    };
    let domain = url
        .split("://")
        .nth(1)
        .unwrap_or("localhost:8090")
        .trim_end_matches('/');
    let ws_url = format!("{}://{}/api/game/ws?room={}", ws_scheme, domain, room);

    let mut request = ws_url.into_client_request()?;
    let cookies_url = reqwest::Url::parse(url)?;
    if let Some(cookie_header) = jar.cookies(&cookies_url) {
        if let Ok(val) = HeaderValue::from_bytes(cookie_header.as_bytes()) {
            let headers: &mut HeaderMap = request.headers_mut();
            headers.insert(COOKIE, val);
        }
    }

    let (ws_stream, _) = connect_async(request).await?;
    println!("WebSocket connected successfully! Listening for hand events...");
    Ok(ws_stream)
}

async fn connect_live_ws(
    url: &str,
    room: &str,
    name: &str,
) -> Result<
    Option<(
        Arc<reqwest::cookie::Jar>,
        reqwest::Client,
        String,
        WebSocketStream<MaybeTlsStream<TcpStream>>,
    )>,
    Box<dyn std::error::Error>,
> {
    let jar = Arc::new(reqwest::cookie::Jar::default());
    let client = reqwest::Client::builder()
        .cookie_provider(Arc::clone(&jar))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .build()?;

    let base_url = url.trim_end_matches('/');
    let login_url = format!("{}/api/auth/guest", base_url);
    let resp = client
        .post(&login_url)
        .json(&serde_json::json!({ "username": name }))
        .send()
        .await?;

    if !resp.status().is_success() {
        eprintln!("Failed guest login: status {}", resp.status());
        return Ok(None);
    }

    let user_info: GuestResponse = resp.json().await?;
    let player_id = user_info.uuid;
    println!(
        "Logged in successfully. Player ID: {}, Chips: {}",
        player_id, user_info.chips
    );

    let join_url = format!("{}/api/game/players?room={}", base_url, room);
    let _ = client
        .post(&join_url)
        .json(&JoinRoomPayload {
            uuid: player_id.clone(),
        })
        .send()
        .await;
    println!(
        "Joined room '{}'. Starting hand & connecting to WebSocket...",
        room
    );

    /* Trigger initial start in case table is waiting */
    let start_url = format!("{}/api/game/start?room={}", base_url, room);
    let _ = client.post(&start_url).send().await;

    let ws_stream = open_ws_stream(url, room, &jar).await?;
    Ok(Some((jar, client, player_id, ws_stream)))
}

fn prefer_all_in_action(
    legal_actions: &[String],
    fallback_action: String,
    fallback_amount: i32,
) -> (String, i32) {
    if legal_actions
        .iter()
        .any(|l| l.eq_ignore_ascii_case("all_in"))
    {
        ("all_in".to_string(), 0)
    } else if legal_actions
        .iter()
        .any(|l| l.eq_ignore_ascii_case("allin"))
    {
        ("allin".to_string(), 0)
    } else {
        (fallback_action, fallback_amount)
    }
}

fn apply_live_commit_guards(
    chosen_action: String,
    raw_amount: i32,
    snapshot: &LiveSnapshot,
    eff_chips: i32,
) -> (String, i32) {
    let hole = &snapshot.hole;
    if (chosen_action == "raise" || chosen_action == "bet") && eff_chips > 0 {
        let max_total = eff_chips + snapshot.my_bet;
        let min_legal_total = if snapshot.current_bet > 0 {
            snapshot.current_bet + snapshot.to_call.max(snapshot.big_blind)
        } else {
            snapshot.big_blind
        };
        let capped = raw_amount.max(min_legal_total).min(max_total);
        let bet_add = (capped - snapshot.my_bet).max(0);
        let remaining_behind = max_total - capped;
        let new_pot = snapshot.pot + bet_add;
        let post_bucket = if !snapshot.board.is_empty() {
            Some(crate::cfr::abstraction::postflop_equity_bucket(
                hole,
                &snapshot.board,
            ))
        } else {
            None
        };
        /* Postflop Air Stack-Risk Guard:
         * Never risk >= 35% of remaining stack on a postflop bet/raise with pure air (bucket < 11),
         * except genuine River nut-flush blocker bluffs when checking is an option. */
        if let Some(b_idx) = post_bucket {
            let river_blocker_bluff = snapshot.board.len() >= 5
                && snapshot.to_call == 0
                && crate::cfr::abstraction::detect_river_nut_blocker(hole, &snapshot.board);
            if b_idx < 11 && bet_add * 20 >= eff_chips * 7 && !river_blocker_bluff {
                if snapshot.to_call == 0 {
                    ("check".to_string(), 0)
                } else if snapshot.to_call <= snapshot.big_blind
                    || snapshot.to_call * 6 <= snapshot.pot
                    || eff_chips <= snapshot.big_blind
                {
                    if snapshot
                        .legal_actions
                        .iter()
                        .any(|l| l.eq_ignore_ascii_case("call"))
                    {
                        ("call".to_string(), 0)
                    } else if snapshot.legal_actions.iter().any(|l| {
                        l.eq_ignore_ascii_case("all_in") || l.eq_ignore_ascii_case("allin")
                    }) {
                        prefer_all_in_action(&snapshot.legal_actions, "call".to_string(), 0)
                    } else {
                        ("fold".to_string(), 0)
                    }
                } else {
                    ("fold".to_string(), 0)
                }
            } else {
                let strong_postflop = b_idx >= 40 || (19..=27).contains(&b_idx);
                let commits_stack = remaining_behind * 4 <= new_pot
                    || remaining_behind <= snapshot.big_blind * 3
                    || (strong_postflop && remaining_behind * 2 <= new_pot);
                if commits_stack {
                    prefer_all_in_action(&snapshot.legal_actions, chosen_action, max_total)
                } else {
                    (chosen_action, capped)
                }
            }
        } else {
            /* Preflop Stack-Commit Guard:
             * Convert raises that leave <= 25% of the new pot behind, leave <= 3 BB behind,
             * or commit >= 55% of total stack on preflop directly into an all-in shove! */
            let commits_stack = remaining_behind * 4 <= new_pot
                || remaining_behind <= snapshot.big_blind * 3
                || capped * 20 >= max_total * 11;
            if commits_stack {
                prefer_all_in_action(&snapshot.legal_actions, chosen_action, max_total)
            } else {
                (chosen_action, capped)
            }
        }
    } else if chosen_action == "call"
        && eff_chips > snapshot.to_call
        && snapshot.to_call > 0
        && !snapshot.board.is_empty()
        && snapshot.board.len() < 5
    {
        /* Postflop Call Stack-Commit Guard:
         * If calling a Flop/Turn raise with a strong hand (bucket >= 45)
         * leaves <= 3 BB and <= 15% of the new pot behind, push all-in immediately! */
        let rem_after_call = eff_chips - snapshot.to_call;
        let new_pot = snapshot.pot + snapshot.to_call;
        let b_idx = crate::cfr::abstraction::postflop_equity_bucket(hole, &snapshot.board);
        if b_idx >= 45 && rem_after_call <= snapshot.big_blind * 3 && rem_after_call * 6 <= new_pot
        {
            if snapshot
                .legal_actions
                .iter()
                .any(|l| l.eq_ignore_ascii_case("all_in") || l.eq_ignore_ascii_case("allin"))
            {
                prefer_all_in_action(&snapshot.legal_actions, chosen_action, raw_amount)
            } else if snapshot
                .legal_actions
                .iter()
                .any(|l| l.eq_ignore_ascii_case("raise"))
            {
                ("raise".to_string(), eff_chips + snapshot.my_bet)
            } else {
                (chosen_action, raw_amount)
            }
        } else {
            (chosen_action, raw_amount)
        }
    } else {
        (chosen_action, raw_amount)
    }
}

fn format_hand_cards(
    hole: &[crate::game::holdem::Card; 2],
    board: &[crate::game::holdem::Card],
) -> (String, String) {
    let hole_str = format!(
        "[{} {}]",
        hole[0].to_string().trim_matches('\''),
        hole[1].to_string().trim_matches('\'')
    );
    let board_str = if board.is_empty() {
        "[]".to_string()
    } else {
        let cards: Vec<String> = board
            .iter()
            .map(|c| c.to_string().trim_matches('\'').to_string())
            .collect();
        format!("[{}]", cards.join(" "))
    };
    (hole_str, board_str)
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
            if !self.current_opp_name.is_empty() {
                if let Some(players) = state.get("players").and_then(|v| v.as_array()) {
                    if players.len() == 2 {
                        let opp_t = self
                            .trackers
                            .entry(self.current_opp_name.clone())
                            .or_default();
                        opp_t.record_action_street_detailed(
                            crate::game::config::FOLD,
                            1,
                            false,
                            0,
                            0,
                        );
                        opp_t.record_preflop_positional(true, false, false);
                        opp_t.end_hand();
                        let _ = opp_t.save_profile(self.profiles_path, &self.current_opp_name);
                    }
                }
            }
            self.table_tracker.reset_hand(0);
            self.last_seen_hand_id = 0;
        }

        if phase.eq_ignore_ascii_case("Waiting") {
            if let Some(players) = state.get("players").and_then(|v| v.as_array()) {
                if players.len() >= 2 {
                    let start_url = format!(
                        "{}/api/game/start?room={}",
                        self.url.trim_end_matches('/'),
                        self.room
                    );
                    let _ = self.client.post(&start_url).send().await;
                }
            }
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

        let msg_hand_id = state
            .get("hand_number")
            .or_else(|| state.get("hand_id"))
            .or_else(|| state.get("hand_count"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if msg_hand_id > 0 {
            self.last_seen_hand_id = msg_hand_id;
        } else if !phase.is_empty() {
            /* Track active hand even if server sends hand_id == 0 */
            self.last_seen_hand_id = self.last_seen_hand_id.max(1);
        }

        /* Detect hand transitions across all table updates (including non-Hero turns at 6-max) */
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

        /* Prevent duplicate action execution on identical state updates */
        if snapshot.action_seq_key == self.last_action_key {
            return Ok(());
        }

        /* Detect hand or street transitions for round history (works even when server hand_id == 0) */
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
            self.round_tracker.infer_opponent_history_only(&snapshot);
        } else {
            self.round_tracker
                .update_street_with_tracker(snapshot.board.len(), Some(active_tracker));
            self.round_tracker
                .infer_and_record_opponent_action(&snapshot, active_tracker);
        }

        let (engine_contributions, raises_this_round) = self
            .round_tracker
            .compute_scaled_subgame_contributions(&snapshot);

        let eff_chips = if snapshot.my_chips > 0 {
            snapshot.my_chips
        } else {
            self.round_tracker.estimated_my_chips
        };
        let strat_clone = Arc::clone(&self.strategy_arc);
        let index_clone = Arc::clone(&self.strategy_index);
        let solver_arc = Arc::clone(&self.subgame_solver);
        let subgame = self.subgame_search;
        let tracker_clone = active_tracker.clone();
        let board_cards_clone = snapshot.board.clone();
        let history_clone = self.round_tracker.live_history;
        let hole = snapshot.hole;
        let legal_actions = snapshot.legal_actions.clone();
        let to_call = snapshot.to_call;
        let pot = snapshot.pot;
        let my_player = snapshot.my_player;
        let active_opponents = snapshot.active_opponents;
        let big_blind = snapshot.big_blind;
        let current_bet_now = snapshot.current_bet;

        let (raw_action, raw_amount, trace) = tokio::task::spawn_blocking(move || {
            let mut ctx = SubgameContext::new();
            decide_action_traced_with_bet(
                &hole,
                &board_cards_clone,
                &legal_actions,
                to_call,
                pot,
                &strat_clone,
                subgame,
                &tracker_clone,
                &history_clone,
                my_player,
                engine_contributions,
                raises_this_round,
                Some(&index_clone),
                Some(&solver_arc),
                Some(&mut ctx),
                active_opponents,
                big_blind,
                eff_chips,
                current_bet_now,
            )
        })
        .await?;

        let (chosen_action, amount) =
            apply_live_commit_guards(raw_action, raw_amount, &snapshot, eff_chips);
        let (hole_str, board_str) = format_hand_cards(&hole, &snapshot.board);

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
        let resp = self
            .client
            .post(&act_url)
            .json(&ActPayload {
                player_id: self.player_id.clone(),
                action: chosen_action.to_string(),
                amount,
            })
            .send()
            .await;

        let Ok(r) = resp else {
            if let Err(e) = resp {
                eprintln!("Act HTTP network error: {}", e);
                self.last_action_key.clear();
            }
            return;
        };

        if r.status().is_success() {
            /* Record bot's own action into live history only after server accepts it */
            self.round_tracker.record_bot_action_with_bet(
                chosen_action,
                amount,
                snapshot.pot,
                snapshot.my_bet,
            );
            if chosen_action == "fold" && self.hand_active {
                self.hero_folded_this_hand = true;
            }
            return;
        }

        let status = r.status();
        let err_text = r.text().await.unwrap_or_default();
        eprintln!(
            "⚠️ Act server notice ({}) [{}]: {}",
            chosen_action, status, err_text
        );
        self.last_action_key.clear();

        /* Automatic fallback retry if server rejected 'check' in favor of 'call',
         * if server rejected 'all_in'/'allin' (fall back to 'call' or total 'raise'),
         * or if server rejected 'raise'/'bet' (re-evaluate non-raise call vs fold instead of blind call!) */
        let has_legal = |tok: &str| {
            snapshot
                .legal_actions
                .iter()
                .any(|a| a.eq_ignore_ascii_case(tok))
        };
        let fallback_act: Option<(String, i32)> = if chosen_action == "check"
            && (err_text.contains("cannot check") || err_text.contains("call"))
        {
            Some(("call".to_string(), 0))
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
                Some(("check".to_string(), 0))
            } else {
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
                    None
                } else {
                    let fb_tracker = self
                        .trackers
                        .get(&self.current_opp_name)
                        .cloned()
                        .unwrap_or_default();
                    let (fb_act, fb_amt, _) = decide_action_traced_with_bet(
                        &snapshot.hole,
                        &snapshot.board,
                        &non_raise_legal,
                        snapshot.to_call,
                        snapshot.pot,
                        &self.strategy_arc,
                        false,
                        &fb_tracker,
                        &self.round_tracker.live_history,
                        snapshot.my_player,
                        engine_contributions,
                        raises_this_round,
                        Some(&self.strategy_index),
                        None,
                        None,
                        snapshot.active_opponents,
                        snapshot.big_blind,
                        my_chips,
                        snapshot.current_bet,
                    );
                    Some((fb_act, fb_amt))
                }
            }
        } else {
            None
        };

        if let Some((fb, fb_amt)) = fallback_act {
            println!(
                "-> Auto-fallback: Retrying action {} (amt: {})",
                fb.to_uppercase(),
                fb_amt
            );
            if let Ok(r_retry) = self
                .client
                .post(&act_url)
                .json(&ActPayload {
                    player_id: self.player_id.clone(),
                    action: fb.clone(),
                    amount: fb_amt,
                })
                .send()
                .await
            {
                if r_retry.status().is_success() {
                    self.round_tracker.record_bot_action_with_bet(
                        &fb,
                        fb_amt,
                        snapshot.pot,
                        snapshot.my_bet,
                    );
                    if fb == "fold" && self.hand_active {
                        self.hero_folded_this_hand = true;
                    }
                }
            }
        }
    }
}

pub async fn run_live_bot(
    url: &str,
    room: &str,
    name: &str,
    strategy_path: &str,
    subgame_search: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== lil-poker-mccfr Live Rust Bot Client ===");
    println!("Server URL: {}", url);
    println!("Room ID:    {}", room);
    println!("Bot Name:   {}", name);

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
        let ws_stream = if let Some(ws) = current_ws.take() {
            backoff_secs = 2;
            ws
        } else {
            match open_ws_stream(url, room, &jar).await {
                Ok(ws) => {
                    backoff_secs = 2;
                    ws
                }
                Err(e) => {
                    eprintln!(
                        "WebSocket reconnect failed ({}). Retrying in {}s...",
                        e, backoff_secs
                    );
                    tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(30);
                    continue;
                }
            }
        };
        let (mut write, mut read) = ws_stream.split();

        while let Some(msg) = read.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("WebSocket error: {}", e);
                    break;
                }
            };
            match msg {
                Message::Ping(payload) => {
                    let _ = write.send(Message::Pong(payload)).await;
                    continue;
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
                        eprintln!("Decision/submit error: {}", e);
                    }
                }
                _ => continue,
            }
        }

        session.save_all_profiles();
        eprintln!(
            "WebSocket disconnected. Reconnecting in {}s...",
            backoff_secs
        );
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(30);
    }
}
