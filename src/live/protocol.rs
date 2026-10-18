/* Live Client Network Protocols, Payloads, and Action Mappings */
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::game::config::*;
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
    map_action_index_round(idx, legal, to_call, pot, is_wet, 2)
}

/* Street-aware CFR Action Index mapping with dynamic River/Turn bet sizing */
pub fn map_action_index_round(
    idx: usize,
    legal: &[String],
    to_call: i32,
    pot: i32,
    is_wet: bool,
    round: u8,
) -> Option<(String, i32)> {
    let has = |a: &str| legal.iter().any(|l| l.eq_ignore_ascii_case(a));

    match idx as u8 {
        FOLD => {
            if to_call == 0 {
                if has("check") {
                    Some(("check".to_string(), 0))
                } else if has("fold") {
                    Some(("fold".to_string(), 0))
                } else {
                    None
                }
            } else if has("fold") {
                Some(("fold".to_string(), 0))
            } else if has("check") {
                Some(("check".to_string(), 0))
            } else {
                None
            }
        }
        CALL_CHECK => {
            if to_call > 0 {
                if has("call") {
                    Some(("call".to_string(), 0))
                } else if has("check") {
                    Some(("check".to_string(), 0))
                } else {
                    None
                }
            } else if has("check") {
                Some(("check".to_string(), 0))
            } else if has("call") {
                Some(("call".to_string(), 0))
            } else {
                None
            }
        }
        RAISE_MIN..=RAISE_HALF_POT => {
            let min_raise = if to_call > 0 {
                (to_call * 2).max(to_call + BIG_BLIND)
            } else {
                BIG_BLIND
            };
            let amt = match idx as u8 {
                RAISE_THIRD_POT => {
                    let s = (pot / 3).max(RAISE_THIRD_POT_FLOOR);
                    if to_call > 0 {
                        (to_call + s).max(min_raise)
                    } else {
                        s.max(min_raise)
                    }
                }
                RAISE_HALF_POT => {
                    let s = if round == 4 {
                        /* River: Polarized 125% pot overbet */
                        (pot * 5 / 4).max(RAISE_HALF_POT_FLOOR_RIVER)
                    } else if round == 3 {
                        /* Turn: 75% pot bet */
                        (pot * 3 / 4).max(RAISE_HALF_POT_FLOOR_TURN)
                    } else if is_wet {
                        (pot * 3 / 4).max(RAISE_HALF_POT_FLOOR_WET)
                    } else {
                        (pot / 2).max(RAISE_HALF_POT_FLOOR_DRY)
                    };
                    if to_call > 0 {
                        (to_call + s).max(min_raise)
                    } else {
                        s.max(min_raise)
                    }
                }
                _ => {
                    if round == 1 && to_call <= BIG_BLIND / 2 {
                        /* Preflop initial open-raise: 2.5x BB (250 chips total) */
                        PREFLOP_OPEN_MIN_TOTAL
                    } else if to_call > 0 {
                        (to_call + RAISE_MIN_AMT).max(min_raise)
                    } else {
                        RAISE_MIN_AMT.max(min_raise)
                    }
                }
            };
            if has("raise") {
                Some(("raise".to_string(), amt))
            } else if has("bet") {
                Some(("bet".to_string(), amt))
            } else if has("all_in") {
                Some(("all_in".to_string(), 0))
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
        ALL_IN => {
            if has("all_in") {
                Some(("all_in".to_string(), 0))
            } else if has("allin") {
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

/* Pseudo-Harmonic mapping between two discrete bet amounts s1 <= s2 for actual bet x:
 * Returns probability of mapping to s1 (lower action) preserving pot odds and EV.
 * Ref: Ganzfried & Sandholm (2013). */
#[inline]
pub fn pseudo_harmonic_prob(x: i32, s1: i32, s2: i32, pot: i32) -> f64 {
    if s2 <= s1 || x <= s1 {
        return 1.0;
    }
    if x >= s2 {
        return 0.0;
    }
    let x_f = x as f64;
    let s1_f = s1 as f64;
    let s2_f = s2 as f64;
    let pot_f = pot.max(1) as f64;

    let w1 = ((s2_f - x_f) / (s2_f - s1_f)) * ((s1_f + pot_f) / (x_f + pot_f));
    w1.clamp(0.0, 1.0)
}

/* Pseudo-Harmonic action translation: maps an arbitrary continuous bet amount into
 * a discrete action index (2: RAISE_MIN, 3: RAISE_THIRD_POT, 4: RAISE_HALF_POT, 5: ALL_IN)
 * preserving pot odds and expectation value.
 * Ref: Ganzfried & Sandholm (2013). */
pub fn pseudo_harmonic_action_mapping<R: rand::Rng>(
    bet_amt: i32,
    pot: i32,
    allin_threshold: i32,
    rng: &mut R,
) -> u8 {
    if bet_amt >= allin_threshold {
        return ALL_IN;
    }

    let s_min = RAISE_MIN_AMT;
    let s_third = (pot / 3).max(RAISE_THIRD_POT_FLOOR);
    let s_half = (pot / 2).max(RAISE_HALF_POT_FLOOR_DRY);

    if bet_amt <= s_min {
        return RAISE_MIN;
    }

    if bet_amt < s_third {
        let p_min = pseudo_harmonic_prob(bet_amt, s_min, s_third, pot);
        if rng.gen::<f64>() <= p_min {
            RAISE_MIN
        } else {
            RAISE_THIRD_POT
        }
    } else if bet_amt < s_half {
        let p_third = pseudo_harmonic_prob(bet_amt, s_third, s_half, pot);
        if rng.gen::<f64>() <= p_third {
            RAISE_THIRD_POT
        } else {
            RAISE_HALF_POT
        }
    } else {
        let s_pot = pot.max(s_half + 100);
        if bet_amt < s_pot {
            let p_half = pseudo_harmonic_prob(bet_amt, s_half, s_pot, pot);
            if rng.gen::<f64>() <= p_half {
                RAISE_HALF_POT
            } else {
                ALL_IN
            }
        } else {
            ALL_IN
        }
    }
}
