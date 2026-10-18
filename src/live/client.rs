/* WebSocket & HTTP Live Bot Client for lil-poker */
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use futures_util::StreamExt;
use reqwest::cookie::CookieStore;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use serde_json::Value;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use super::decision::decide_action;
use super::protocol::{ActPayload, GuestResponse, JoinRoomPayload};
use super::state::{parse_live_state, process_showdown, LiveRoundTracker};
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

    /* 2. Guest Login */
    let jar = Arc::new(reqwest::cookie::Jar::default());
    let client = reqwest::Client::builder()
        .cookie_provider(Arc::clone(&jar))
        .build()?;

    let login_url = format!("{}/api/auth/guest", url.trim_end_matches('/'));
    let resp = client
        .post(&login_url)
        .json(&serde_json::json!({ "username": name }))
        .send()
        .await?;

    if !resp.status().is_success() {
        eprintln!("Failed guest login: status {}", resp.status());
        return Ok(());
    }

    let user_info: GuestResponse = resp.json().await?;
    let player_id = user_info.uuid;
    println!(
        "Logged in successfully. Player ID: {}, Chips: {}",
        player_id, user_info.chips
    );

    /* 3. Join Room */
    let join_url = format!(
        "{}/api/game/players?room={}",
        url.trim_end_matches('/'),
        room
    );
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
    let start_url = format!("{}/api/game/start?room={}", url.trim_end_matches('/'), room);
    let _ = client.post(&start_url).send().await;

    /* 4. Connect WebSocket */
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

    let (_, mut read) = ws_stream.split();
    let mut tracker = OpponentTracker::new();
    let mut last_action_key = String::new();
    let subgame_solver = Arc::new(SubgameSolver::default());
    let strategy_arc = Arc::new(strategy_map);

    let mut round_tracker = LiveRoundTracker::new();

    /* 5. Main Game Loop */
    while let Some(msg) = read.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                eprintln!("WebSocket error: {}", e);
                break;
            }
        };

        if !msg.is_text() {
            continue;
        }

        let text = msg.to_text()?;
        let state: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let phase = state.get("phase").and_then(|v| v.as_str()).unwrap_or("");
        if phase.eq_ignore_ascii_case("Showdown") || phase.eq_ignore_ascii_case("Waiting") {
            if phase.eq_ignore_ascii_case("Showdown") {
                process_showdown(
                    &state,
                    &player_id,
                    &mut tracker,
                    &round_tracker.live_history[3],
                    round_tracker.last_opp_bet,
                );
            }
            tracker.end_hand();
            round_tracker.reset_hand(0);
            last_action_key.clear();

            if phase.eq_ignore_ascii_case("Waiting") {
                if let Some(players) = state.get("players").and_then(|v| v.as_array()) {
                    if players.len() >= 2 {
                        let start_url =
                            format!("{}/api/game/start?room={}", url.trim_end_matches('/'), room);
                        let _ = client.post(&start_url).send().await;
                    }
                }
            }
            continue;
        }

        let snapshot = match parse_live_state(&state, &player_id) {
            Some(s) => s,
            None => continue,
        };

        /* Prevent duplicate action execution on identical state updates */
        if snapshot.action_seq_key == last_action_key {
            continue;
        }

        /* Detect hand or street transitions for round history */
        if snapshot.hand_id != round_tracker.current_hand_id {
            round_tracker.reset_hand(snapshot.hand_id);
        }

        round_tracker.update_street(snapshot.board.len());
        round_tracker.infer_and_record_opponent_action(&snapshot, &mut tracker);

        let (engine_contributions, raises_this_round) =
            round_tracker.compute_engine_contributions(&snapshot);

        let strat_clone = Arc::clone(&strategy_arc);
        let solver_arc = Arc::clone(&subgame_solver);
        let subgame = subgame_search;
        let tracker_clone = tracker.clone();
        let board_cards_clone = snapshot.board.clone();
        let history_clone = round_tracker.live_history;
        let hole = snapshot.hole;
        let legal_actions = snapshot.legal_actions.clone();
        let to_call = snapshot.to_call;
        let pot = snapshot.pot;
        let my_player = snapshot.my_player;

        let (chosen_action, amount) = tokio::task::spawn_blocking(move || {
            let mut ctx = SubgameContext::new();
            decide_action(
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
                None,
                Some(&solver_arc),
                Some(&mut ctx),
            )
        })
        .await?;

        /* Record bot's own action into live history */
        round_tracker.record_bot_action(&chosen_action, amount, snapshot.pot);

        let hole_str = format!(
            "[{} {}]",
            hole[0].to_string().trim_matches('\''),
            hole[1].to_string().trim_matches('\'')
        );
        let board_str = if snapshot.board.is_empty() {
            "[]".to_string()
        } else {
            let cards: Vec<String> = snapshot
                .board
                .iter()
                .map(|c| c.to_string().trim_matches('\'').to_string())
                .collect();
            format!("[{}]", cards.join(" "))
        };

        println!(
            "-> Action: {:<5} (amt: {:>2}) | Hole: {:<7} | Board: {:<15} | Phase: {}",
            chosen_action.to_uppercase(),
            amount,
            hole_str,
            board_str,
            snapshot.phase
        );

        last_action_key = snapshot.action_seq_key;

        let act_url = format!("{}/api/game/act?room={}", url.trim_end_matches('/'), room);
        let resp = client
            .post(&act_url)
            .json(&ActPayload {
                player_id: player_id.clone(),
                action: chosen_action.clone(),
                amount,
            })
            .send()
            .await;

        if let Ok(r) = resp {
            if !r.status().is_success() {
                let status = r.status();
                let err_text = r.text().await.unwrap_or_default();
                eprintln!(
                    "⚠️ Act server notice ({}) [{}]: {}",
                    chosen_action, status, err_text
                );
                last_action_key.clear();

                /* Automatic fallback retry if server rejected 'check' in favor of 'call' */
                if chosen_action == "check"
                    && (err_text.contains("cannot check") || err_text.contains("call"))
                {
                    println!("-> Auto-fallback: Retrying action CALL (amt: 0)");
                    let _ = client
                        .post(&act_url)
                        .json(&ActPayload {
                            player_id: player_id.clone(),
                            action: "call".to_string(),
                            amount: 0,
                        })
                        .send()
                        .await;
                }
            }
        } else if let Err(e) = resp {
            eprintln!("Act HTTP network error: {}", e);
            last_action_key.clear();
        }
    }

    Ok(())
}
