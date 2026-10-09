/* Live Client Network Protocols, Payloads, and Action Mappings */
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::game::config::{
    RAISE_HALF_POT_FLOOR_DRY, RAISE_HALF_POT_FLOOR_WET, RAISE_MIN_AMT, RAISE_THIRD_POT_FLOOR,
};
use crate::game::holdem::{Card, Rank, Suit};

#[derive(Deserialize, Debug, Clone)]
pub struct GuestResponse {
    pub uuid: String,
    #[serde(default)]
    pub chips: i32,
}

#[derive(Serialize, Debug, Clone)]
pub struct JoinRoomPayload {
    pub uuid: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct ActPayload {
    pub player_id: String,
    pub action: String,
    pub amount: i32,
}

pub fn parse_json_i32(v: Option<&Value>) -> i32 {
    let Some(val) = v else {
        return 0;
    };
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
pub fn parse_card_str(s: &str) -> Option<Card> {
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
        "2" => Rank::Two,
        "3" => Rank::Three,
        "4" => Rank::Four,
        "5" => Rank::Five,
        "6" => Rank::Six,
        "7" => Rank::Seven,
        "8" => Rank::Eight,
        "9" => Rank::Nine,
        "10" | "T" => Rank::Ten,
        "J" => Rank::Jack,
        "Q" => Rank::Queen,
        "K" => Rank::King,
        "A" => Rank::Ace,
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

/* Map CFR Action Index (0: FOLD, 1: CALL/CHECK, 2: RAISE_MIN, 3: RAISE_THIRD_POT, 4: RAISE_HALF_POT, 5: ALL_IN) to server legal string */
pub fn map_action_index(
    idx: usize,
    legal: &[String],
    to_call: i32,
    pot: i32,
    is_wet: bool,
) -> Option<(String, i32)> {
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
        2..=4 => {
            let amt = match idx {
                3 => (pot / 3).max(RAISE_THIRD_POT_FLOOR),
                4 => {
                    if is_wet {
                        (pot * 3 / 4).max(RAISE_HALF_POT_FLOOR_WET)
                    } else {
                        (pot / 2).max(RAISE_HALF_POT_FLOOR_DRY)
                    }
                }
                _ => RAISE_MIN_AMT,
            };
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
        5 => {
            if has("allin") {
                Some(("allin".to_string(), 0))
            } else if has("raise") {
                Some(("raise".to_string(), (pot * 2).max(RAISE_MIN_AMT * 2)))
            } else if has("bet") {
                Some(("bet".to_string(), (pot * 2).max(RAISE_MIN_AMT * 2)))
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
