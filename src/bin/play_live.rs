/*
 * Native Rust Live Client for lil-poker.
 * Connects directly to lil-poker REST API + WebSocket server,
 * loads abstract MCCFR strategy model, and plays in real-time.
 */

use clap::Parser;
use futures_util::StreamExt;
use reqwest::cookie::CookieStore;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use lil_poker_mccfr::cfr::abstraction::{get_holdem_infoset_key, postflop_equity_bucket, preflop_bucket};
use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
use lil_poker_mccfr::cfr::subgame::SubgameSolver;
use lil_poker_mccfr::game::holdem::{Card, Rank, Suit};

#[derive(Parser, Debug)]
#[command(name = "play_live", author, version, about = "Live Native Rust Bot for lil-poker")]
struct Args {
    #[arg(short, long, default_value = "http://localhost:8090")]
    url: String,

    #[arg(short, long)]
    room: String,

    #[arg(short, long, default_value = "Rust_CFR_Bot")]
    name: String,

    #[arg(short, long, default_value = "models/holdem_abstract_strategy.json")]
    strategy: String,

    #[arg(long, default_value_t = false)]
    subgame_search: bool,
}

#[derive(Deserialize)]
struct GuestResponse {
    uuid: String,
    #[serde(default)]
    chips: i32,
}

#[derive(Serialize)]
struct JoinRoomPayload {
    uuid: String,
}

#[derive(Serialize)]
struct ActPayload {
    player_id: String,
    action: String,
    amount: i32,
}

fn parse_json_i32(v: Option<&Value>) -> i32 {
    let Some(val) = v else { return 0; };
    if let Some(i) = val.as_i64() {
        i as i32
    } else if let Some(f) = val.as_f64() {
        f as i32
    } else if let Some(s) = val.as_str() {
        s.parse::<i32>().unwrap_or(0)
    } else {
        0
    }
}

/* Parse Card from server string format ("As", "Ah", "A♠", "10c", etc.) */
fn parse_card_str(s: &str) -> Option<Card> {
    let s = s.trim_matches('\'').trim();
    if s.len() < 2 {
        return None;
    }

    let (rank_str, suit_char) = if s.starts_with("10") {
        ("10", s.chars().nth(2)?)
    } else {
        (&s[0..1], s.chars().nth(1)?)
    };

    let rank = match rank_str {
        "2" => Rank::Two, "3" => Rank::Three, "4" => Rank::Four, "5" => Rank::Five,
        "6" => Rank::Six, "7" => Rank::Seven, "8" => Rank::Eight, "9" => Rank::Nine,
        "10" | "T" => Rank::Ten, "J" => Rank::Jack, "Q" => Rank::Queen,
        "K" => Rank::King, "A" => Rank::Ace,
        _ => return None,
    };

    let suit = match suit_char {
        'c' | 'C' | '♣' => Suit::Clubs,
        'd' | 'D' | '♦' => Suit::Diamonds,
        'h' | 'H' | '♥' => Suit::Hearts,
        's' | 'S' | '♠' => Suit::Spades,
        _ => return None,
    };

    Some(Card { rank, suit })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("=== lil-poker-mccfr Live Rust Bot Client ===");
    println!("Server URL: {}", args.url);
    println!("Room ID:    {}", args.room);
    println!("Bot Name:   {}", args.name);

    /* 1. Load Strategy Model */
    let strategy_map: HashMap<String, Vec<f64>> = if std::path::Path::new(&args.strategy).exists() {
        println!("Loading strategy model from {}...", args.strategy);
        let file = File::open(&args.strategy)?;
        let reader = BufReader::new(file);
        serde_json::from_reader(reader)?
    } else {
        println!("WARNING: Strategy file {} not found! Using random play.", args.strategy);
        HashMap::new()
    };
    println!("Loaded strategy with {} infosets.", strategy_map.len());

    /* 2. Guest Login */
    let jar = Arc::new(reqwest::cookie::Jar::default());
    let client = reqwest::Client::builder()
        .cookie_provider(Arc::clone(&jar))
        .build()?;

    let login_url = format!("{}/api/auth/guest", args.url.trim_end_matches('/'));
    let resp = client
        .post(&login_url)
        .json(&serde_json::json!({ "username": args.name }))
        .send()
        .await?;

    if !resp.status().is_success() {
        eprintln!("Failed guest login: status {}", resp.status());
        return Ok(());
    }

    let user_info: GuestResponse = resp.json().await?;
    let player_id = user_info.uuid;
    println!("Logged in successfully. Player ID: {}, Chips: {}", player_id, user_info.chips);

    /* 3. Join Room */
    let join_url = format!("{}/api/game/players?room={}", args.url.trim_end_matches('/'), args.room);
    let _ = client
        .post(&join_url)
        .json(&JoinRoomPayload { uuid: player_id.clone() })
        .send()
        .await;
    println!("Joined room '{}'. Starting hand & connecting to WebSocket...", args.room);

    /* Trigger initial start in case table is waiting */
    let start_url = format!("{}/api/game/start?room={}", args.url.trim_end_matches('/'), args.room);
    let _ = client.post(&start_url).send().await;

    /* 4. Connect WebSocket */
    let ws_scheme = if args.url.starts_with("https://") { "wss" } else { "ws" };
    let domain = args.url.split("://").nth(1).unwrap_or("localhost:8090").trim_end_matches('/');
    let ws_url = format!("{}://{}/api/game/ws?room={}", ws_scheme, domain, args.room);

    let mut request = ws_url.into_client_request()?;
    let cookies_url = reqwest::Url::parse(&args.url)?;
    if let Some(cookie_header) = jar.cookies(&cookies_url) {
        if let Ok(val) = HeaderValue::from_bytes(cookie_header.as_bytes()) {
            let headers: &mut HeaderMap = request.headers_mut();
            headers.insert(COOKIE, val);
        }
    }

    let (ws_stream, _) = connect_async(request).await?;
    println!("WebSocket connected successfully! Listening for hand events...",);

    let (_, mut read) = ws_stream.split();
    let mut tracker = OpponentTracker::new();
    let mut last_action_key = String::new();
    let strategy_map = Arc::new(strategy_map);

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
            tracker.end_hand();
            if phase.eq_ignore_ascii_case("Waiting") {
                if let Some(players) = state.get("players").and_then(|v| v.as_array()) {
                    if players.len() >= 2 {
                        let start_url = format!("{}/api/game/start?room={}", args.url.trim_end_matches('/'), args.room);
                        let _ = client.post(&start_url).send().await;
                    }
                }
            }
            continue;
        }

        let active_id = state.get("active_player_id").and_then(|v| v.as_str()).unwrap_or("");
        if !active_id.eq_ignore_ascii_case(&player_id) {
            continue;
        }

        /* Extract player hole cards */
        let mut hole_cards: Vec<Card> = Vec::new();
        if let Some(players) = state.get("players").and_then(|v| v.as_array()) {
            for p in players {
                let p_id = p.get("id").or_else(|| p.get("uuid")).and_then(|v| v.as_str()).unwrap_or("");
                if p_id.eq_ignore_ascii_case(&player_id) {
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
            continue;
        }

        let hole: [Card; 2] = [hole_cards[0], hole_cards[1]];

        /* Extract board cards */
        let mut board_cards: Vec<Card> = Vec::new();
        if let Some(board) = state.get("board").and_then(|v| v.as_array()) {
            for c in board {
                if let Some(cs) = c.as_str() {
                    if let Some(parsed) = parse_card_str(cs) {
                        board_cards.push(parsed);
                    }
                }
            }
        }

        /* Extract legal actions */
        let legal_actions: Vec<String> = state
            .get("legal_actions")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_lowercase())).collect())
            .unwrap_or_else(|| vec!["check".to_string(), "fold".to_string()]);

        let mut max_bet_on_table = 0i32;
        let mut my_bet = 0i32;

        if let Some(players) = state.get("players").and_then(|v| v.as_array()) {
            for p in players {
                let p_bet = parse_json_i32(p.get("bet").or_else(|| p.get("current_bet")));
                if p_bet > max_bet_on_table {
                    max_bet_on_table = p_bet;
                }

                let p_id = p.get("id").or_else(|| p.get("uuid")).and_then(|v| v.as_str()).unwrap_or("");
                if p_id.eq_ignore_ascii_case(&player_id) {
                    my_bet = p_bet;
                }
            }
        }

        let state_bet = parse_json_i32(state.get("current_bet").or_else(|| state.get("currentBet")).or_else(|| state.get("call_amount")));
        let current_bet = max_bet_on_table.max(state_bet);
        let to_call = (current_bet - my_bet).max(0);
        let pot = parse_json_i32(state.get("pot"));
        let hist_len = state.get("history").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);

        /* Prevent duplicate action execution on identical state updates */
        let hand_id = state.get("hand_number").or_else(|| state.get("hand_id")).and_then(|v| v.as_u64()).unwrap_or(0);
        let action_seq_key = format!("{}:{}:{}:{}:{}:{}:{}", hand_id, phase, board_cards.len(), current_bet, my_bet, pot, hist_len);
        if action_seq_key == last_action_key {
            continue;
        }

        let strat_map = Arc::clone(&strategy_map);
        let subgame = args.subgame_search;
        let tracker_clone = tracker.clone();
        let board_cards_clone = board_cards.clone();

        let (chosen_action, amount) = tokio::task::spawn_blocking(move || {
            decide_action(
                &hole,
                &board_cards_clone,
                &legal_actions,
                to_call,
                &strat_map,
                subgame,
                &tracker_clone,
            )
        })
        .await?;

        let hole_str = format!("[{} {}]", hole[0].to_string().trim_matches('\''), hole[1].to_string().trim_matches('\''));
        let board_str = if board_cards.is_empty() {
            "[]".to_string()
        } else {
            let cards: Vec<String> = board_cards.iter().map(|c| c.to_string().trim_matches('\'').to_string()).collect();
            format!("[{}]", cards.join(" "))
        };

        println!(
            "-> Action: {:<5} (amt: {:>2}) | Hole: {:<7} | Board: {:<15} | Phase: {}",
            chosen_action.to_uppercase(),
            amount,
            hole_str,
            board_str,
            phase
        );

        last_action_key = action_seq_key;

        let act_url = format!("{}/api/game/act?room={}", args.url.trim_end_matches('/'), args.room);
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
                eprintln!("⚠️ Act server notice ({}) [{}]: {}", chosen_action, status, err_text);
                last_action_key.clear();

                /* Automatic fallback retry if server rejected 'check' in favor of 'call' */
                if chosen_action == "check" && (err_text.contains("cannot check") || err_text.contains("call")) {
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
            eprintln!("⚠️ Act HTTP network error: {}", e);
            last_action_key.clear();
        }
    }

    Ok(())
}

fn decide_action(
    hole: &[Card; 2],
    board: &[Card],
    legal: &[String],
    to_call: i32,
    strategy_map: &HashMap<String, Vec<f64>>,
    subgame_search: bool,
    _tracker: &OpponentTracker,
) -> (String, i32) {
    let round = match board.len() {
        0 => 1,
        3 => 2,
        4 => 3,
        _ => 4,
    };
    let history_dummy: [Vec<u8>; 4] = [vec![], vec![], vec![], vec![]];

    /* 1. Real-time Subgame Search (Turn & River) */
    if subgame_search && board.len() >= 3 {
        let solver = SubgameSolver::new(1500);
        let probs = solver.solve(hole, board, round, &history_dummy, 0);
        if let Some((max_idx, _)) = probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        {
            if let Some(res) = map_action_index(max_idx, legal, to_call) {
                return res;
            }
        }
    }

    /* 2. Abstract Strategy Model Lookup */
    let exact_key = get_holdem_infoset_key(hole, board, round, &history_dummy);

    let probs_opt = if let Some(p) = strategy_map.get(&exact_key) {
        Some(p.clone())
    } else {
        let prefix = if round == 1 {
            let (_, name) = preflop_bucket(hole[0], hole[1]);
            format!("P:{}/", name)
        } else {
            let bucket = postflop_equity_bucket(hole, board);
            let r_code = match round { 2 => "F", 3 => "T", 4 => "R", _ => "X" };
            format!("{}:B{:02}/", r_code, bucket)
        };

        let matches: Vec<&Vec<f64>> = strategy_map
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, v)| v)
            .collect();

        if !matches.is_empty() {
            let n = matches.len() as f64;
            let mut avg = vec![0.0f64; 4];
            for vec in matches {
                for (i, &val) in vec.iter().enumerate().take(4) {
                    avg[i] += val / n;
                }
            }
            Some(avg)
        } else {
            None
        }
    };

    if let Some(probs) = probs_opt {
        if probs.len() >= 4 {
            let max_idx = probs
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx)
                .unwrap_or(1);

            if let Some(res) = map_action_index(max_idx, legal, to_call) {
                return res;
            }
        }
    }

    /* 3. Fallback Heuristics */
    if board.is_empty() {
        let (pf_idx, _pf_name) = preflop_bucket(hole[0], hole[1]);
        if pf_idx < 40 {
            if let Some(res) = map_action_index(2, legal, to_call) { return res; }
        } else if pf_idx < 110 {
            if let Some(res) = map_action_index(1, legal, to_call) { return res; }
        }
    } else {
        let bucket = postflop_equity_bucket(hole, board);
        if bucket >= 30 {
            if let Some(res) = map_action_index(2, legal, to_call) { return res; }
        } else if bucket >= 12 {
            if let Some(res) = map_action_index(1, legal, to_call) { return res; }
        }
    }

    /* Safe default: Check -> Call -> Fold */
    let has = |a: &str| legal.iter().any(|l| l.eq_ignore_ascii_case(a));
    if to_call == 0 && has("check") {
        ("check".to_string(), 0)
    } else if has("call") {
        ("call".to_string(), 0)
    } else if has("check") {
        ("check".to_string(), 0)
    } else {
        ("fold".to_string(), 0)
    }
}

/* Map CFR Action Index (0: FOLD, 1: CALL/CHECK, 2: RAISE_MIN, 3: RAISE_HALF_POT) to server legal string */
fn map_action_index(idx: usize, legal: &[String], to_call: i32) -> Option<(String, i32)> {
    let has = |a: &str| legal.iter().any(|l| l.eq_ignore_ascii_case(a));

    match idx {
        0 => {
            if to_call == 0 && has("check") {
                Some(("check".to_string(), 0))
            } else if has("fold") {
                Some(("fold".to_string(), 0))
            } else if has("check") {
                Some(("check".to_string(), 0))
            } else {
                None
            }
        }
        1 => {
            if to_call > 0 && has("call") {
                Some(("call".to_string(), 0))
            } else if has("check") {
                Some(("check".to_string(), 0))
            } else if has("call") {
                Some(("call".to_string(), 0))
            } else {
                None
            }
        }
        2 | 3 => {
            let amt = if idx == 3 { 80 } else { 40 };
            if has("raise") {
                Some(("raise".to_string(), amt))
            } else if has("bet") {
                Some(("bet".to_string(), amt))
            } else if has("allin") {
                Some(("allin".to_string(), 0))
            } else if to_call > 0 && has("call") {
                Some(("call".to_string(), 0))
            } else if has("check") {
                Some(("check".to_string(), 0))
            } else {
                None
            }
        }
        _ => None,
    }
}
