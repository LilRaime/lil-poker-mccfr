/*! Live Client HTTP/WebSocket Transport and Strategy Loader */
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;

use reqwest::cookie::CookieStore;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::game::config::HOLDEM_NUM_ACTIONS;
use crate::game::holdem::Card;
use crate::live::protocol::{GuestResponse, JoinRoomPayload};

pub(super) type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/** Load live strategy table from .bin/.bincode or .json file */
pub fn load_live_strategy(path: &str) -> HashMap<String, Vec<f64>> {
    if std::path::Path::new(path).exists() {
        println!("Loading strategy model from {path}...");
        if let Ok(file) = File::open(path) {
            let mut reader = BufReader::new(file);
            if std::path::Path::new(path)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("bin") || e.eq_ignore_ascii_case("bincode"))
            {
                if let Ok(raw_f32) = bincode::deserialize_from::<
                    _,
                    HashMap<String, [f32; HOLDEM_NUM_ACTIONS]>,
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
    println!("WARNING: Strategy file {path} not found! Using random play.");
    HashMap::new()
}

pub(super) async fn open_ws_stream(
    url: &str,
    room: &str,
    jar: &reqwest::cookie::Jar,
) -> Result<WsStream, Box<dyn std::error::Error>> {
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
    let ws_url = format!("{ws_scheme}://{domain}/api/game/ws?room={room}");

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

pub(super) async fn connect_live_ws(
    url: &str,
    room: &str,
    name: &str,
) -> Result<
    Option<(Arc<reqwest::cookie::Jar>, reqwest::Client, String, WsStream)>,
    Box<dyn std::error::Error>,
> {
    let jar = Arc::new(reqwest::cookie::Jar::default());
    let client = reqwest::Client::builder()
        .cookie_provider(Arc::clone(&jar))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .build()?;

    let base_url = url.trim_end_matches('/');
    let login_url = format!("{base_url}/api/auth/guest");
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

    let join_url = format!("{base_url}/api/game/players?room={room}");
    let _ = client
        .post(&join_url)
        .json(&JoinRoomPayload {
            uuid: player_id.clone(),
        })
        .send()
        .await;
    println!("Joined room '{room}'. Starting hand & connecting to WebSocket...");

    /* Trigger initial start in case table is waiting */
    let start_url = format!("{base_url}/api/game/start?room={room}");
    let _ = client.post(&start_url).send().await;

    let ws_stream = open_ws_stream(url, room, &jar).await?;
    Ok(Some((jar, client, player_id, ws_stream)))
}

pub(super) async fn next_ws_stream(
    url: &str,
    room: &str,
    jar: &reqwest::cookie::Jar,
    current_ws: &mut Option<WsStream>,
    backoff_secs: &mut u64,
) -> WsStream {
    loop {
        if let Some(ws) = current_ws.take() {
            *backoff_secs = 2;
            return ws;
        }
        match open_ws_stream(url, room, jar).await {
            Ok(ws) => {
                *backoff_secs = 2;
                return ws;
            }
            Err(e) => {
                eprintln!(
                    "WebSocket reconnect failed ({}). Retrying in {}s...",
                    e, *backoff_secs
                );
                tokio::time::sleep(Duration::from_secs(*backoff_secs)).await;
                *backoff_secs = (*backoff_secs * 2).min(30);
            }
        }
    }
}

pub(super) fn format_hand_cards(hole: [Card; 2], board: &[Card]) -> (String, String) {
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
