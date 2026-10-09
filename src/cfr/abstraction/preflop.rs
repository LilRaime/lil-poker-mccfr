/*
 * Canonical Preflop Hand Indexing and Groups (Pairs, Suited, Offsuited).
 */

use crate::game::holdem::{Card, Rank};

/* Canonical Preflop Hand Index (0..168) & Name ("AA", "AKs", "AKo", etc.) */
pub fn preflop_bucket(c1: Card, c2: Card) -> (usize, &'static str) {
    let (r1, r2) = if (c1.rank as u8) >= (c2.rank as u8) {
        (c1.rank, c2.rank)
    } else {
        (c2.rank, c1.rank)
    };

    let is_pair = r1 == r2;
    let is_suited = c1.suit == c2.suit;

    if is_pair {
        let idx = (Rank::Ace as usize) - (r1 as usize);
        let names = [
            "AA", "KK", "QQ", "JJ", "TT", "99", "88", "77", "66", "55", "44", "33", "22",
        ];
        (idx, names[idx])
    } else if is_suited {
        /* 78 Suited Hands: AKs, AQs ... 32s */
        let high = (Rank::Ace as usize) - (r1 as usize);
        let low = (Rank::Ace as usize) - (r2 as usize);
        /* Triangle index calculation */
        let offset = 13;
        let mut pair_count = 0;
        for h in 0..high {
            pair_count += 12 - h;
        }
        let idx = offset + pair_count + (low - high - 1);
        (idx, suited_name(r1, r2))
    } else {
        /* 78 Offsuited Hands: AKo, AQo ... 32o */
        let high = (Rank::Ace as usize) - (r1 as usize);
        let low = (Rank::Ace as usize) - (r2 as usize);
        let offset = 13 + 78;
        let mut pair_count = 0;
        for h in 0..high {
            pair_count += 12 - h;
        }
        let idx = offset + pair_count + (low - high - 1);
        (idx, offsuited_name(r1, r2))
    }
}

#[allow(dead_code)]
fn rank_char(r: Rank) -> &'static str {
    match r {
        Rank::Ace => "A",
        Rank::King => "K",
        Rank::Queen => "Q",
        Rank::Jack => "J",
        Rank::Ten => "T",
        Rank::Nine => "9",
        Rank::Eight => "8",
        Rank::Seven => "7",
        Rank::Six => "6",
        Rank::Five => "5",
        Rank::Four => "4",
        Rank::Three => "3",
        Rank::Two => "2",
    }
}

fn suited_name(r1: Rank, r2: Rank) -> &'static str {
    match (r1, r2) {
        (Rank::Ace, Rank::King) => "AKs",
        (Rank::Ace, Rank::Queen) => "AQs",
        (Rank::Ace, Rank::Jack) => "AJs",
        (Rank::Ace, Rank::Ten) => "ATs",
        (Rank::Ace, Rank::Nine) => "A9s",
        (Rank::Ace, Rank::Eight) => "A8s",
        (Rank::Ace, Rank::Seven) => "A7s",
        (Rank::Ace, Rank::Six) => "A6s",
        (Rank::Ace, Rank::Five) => "A5s",
        (Rank::Ace, Rank::Four) => "A4s",
        (Rank::Ace, Rank::Three) => "A3s",
        (Rank::Ace, Rank::Two) => "A2s",
        (Rank::King, Rank::Queen) => "KQs",
        (Rank::King, Rank::Jack) => "KJs",
        (Rank::King, Rank::Ten) => "KTs",
        (Rank::King, Rank::Nine) => "K9s",
        (Rank::King, Rank::Eight) => "K8s",
        (Rank::King, Rank::Seven) => "K7s",
        (Rank::King, Rank::Six) => "K6s",
        (Rank::King, Rank::Five) => "K5s",
        (Rank::King, Rank::Four) => "K4s",
        (Rank::King, Rank::Three) => "K3s",
        (Rank::King, Rank::Two) => "K2s",
        (Rank::Queen, Rank::Jack) => "QJs",
        (Rank::Queen, Rank::Ten) => "QTs",
        (Rank::Queen, Rank::Nine) => "Q9s",
        (Rank::Queen, Rank::Eight) => "Q8s",
        (Rank::Queen, Rank::Seven) => "Q7s",
        (Rank::Queen, Rank::Six) => "Q6s",
        (Rank::Queen, Rank::Five) => "Q5s",
        (Rank::Queen, Rank::Four) => "Q4s",
        (Rank::Queen, Rank::Three) => "Q3s",
        (Rank::Queen, Rank::Two) => "Q2s",
        (Rank::Jack, Rank::Ten) => "JTs",
        (Rank::Jack, Rank::Nine) => "J9s",
        (Rank::Jack, Rank::Eight) => "J8s",
        (Rank::Jack, Rank::Seven) => "J7s",
        (Rank::Jack, Rank::Six) => "J6s",
        (Rank::Jack, Rank::Five) => "J5s",
        (Rank::Jack, Rank::Four) => "J4s",
        (Rank::Jack, Rank::Three) => "J3s",
        (Rank::Jack, Rank::Two) => "J2s",
        (Rank::Ten, Rank::Nine) => "T9s",
        (Rank::Ten, Rank::Eight) => "T8s",
        (Rank::Ten, Rank::Seven) => "T7s",
        (Rank::Ten, Rank::Six) => "T6s",
        (Rank::Ten, Rank::Five) => "T5s",
        (Rank::Ten, Rank::Four) => "T4s",
        (Rank::Ten, Rank::Three) => "T3s",
        (Rank::Ten, Rank::Two) => "T2s",
        (Rank::Nine, Rank::Eight) => "98s",
        (Rank::Nine, Rank::Seven) => "97s",
        (Rank::Nine, Rank::Six) => "96s",
        (Rank::Nine, Rank::Five) => "95s",
        (Rank::Nine, Rank::Four) => "94s",
        (Rank::Nine, Rank::Three) => "93s",
        (Rank::Nine, Rank::Two) => "92s",
        (Rank::Eight, Rank::Seven) => "87s",
        (Rank::Eight, Rank::Six) => "86s",
        (Rank::Eight, Rank::Five) => "85s",
        (Rank::Eight, Rank::Four) => "84s",
        (Rank::Eight, Rank::Three) => "83s",
        (Rank::Eight, Rank::Two) => "82s",
        (Rank::Seven, Rank::Six) => "76s",
        (Rank::Seven, Rank::Five) => "75s",
        (Rank::Seven, Rank::Four) => "74s",
        (Rank::Seven, Rank::Three) => "73s",
        (Rank::Seven, Rank::Two) => "72s",
        (Rank::Six, Rank::Five) => "65s",
        (Rank::Six, Rank::Four) => "64s",
        (Rank::Six, Rank::Three) => "63s",
        (Rank::Six, Rank::Two) => "62s",
        (Rank::Five, Rank::Four) => "54s",
        (Rank::Five, Rank::Three) => "53s",
        (Rank::Five, Rank::Two) => "52s",
        (Rank::Four, Rank::Three) => "43s",
        (Rank::Four, Rank::Two) => "42s",
        (Rank::Three, Rank::Two) => "32s",
        _ => "XXs",
    }
}

fn offsuited_name(r1: Rank, r2: Rank) -> &'static str {
    match (r1, r2) {
        (Rank::Ace, Rank::King) => "AKo",
        (Rank::Ace, Rank::Queen) => "AQo",
        (Rank::Ace, Rank::Jack) => "AJo",
        (Rank::Ace, Rank::Ten) => "ATo",
        (Rank::Ace, Rank::Nine) => "A9o",
        (Rank::Ace, Rank::Eight) => "A8o",
        (Rank::Ace, Rank::Seven) => "A7o",
        (Rank::Ace, Rank::Six) => "A6o",
        (Rank::Ace, Rank::Five) => "A5o",
        (Rank::Ace, Rank::Four) => "A4o",
        (Rank::Ace, Rank::Three) => "A3o",
        (Rank::Ace, Rank::Two) => "A2o",
        (Rank::King, Rank::Queen) => "KQo",
        (Rank::King, Rank::Jack) => "KJo",
        (Rank::King, Rank::Ten) => "KTo",
        (Rank::King, Rank::Nine) => "K9o",
        (Rank::King, Rank::Eight) => "K8o",
        (Rank::King, Rank::Seven) => "K7o",
        (Rank::King, Rank::Six) => "K6o",
        (Rank::King, Rank::Five) => "K5o",
        (Rank::King, Rank::Four) => "K4o",
        (Rank::King, Rank::Three) => "K3o",
        (Rank::King, Rank::Two) => "K2o",
        (Rank::Queen, Rank::Jack) => "QJo",
        (Rank::Queen, Rank::Ten) => "QTo",
        (Rank::Queen, Rank::Nine) => "Q9o",
        (Rank::Queen, Rank::Eight) => "Q8o",
        (Rank::Queen, Rank::Seven) => "Q7o",
        (Rank::Queen, Rank::Six) => "Q6o",
        (Rank::Queen, Rank::Five) => "Q5o",
        (Rank::Queen, Rank::Four) => "Q4o",
        (Rank::Queen, Rank::Three) => "Q3o",
        (Rank::Queen, Rank::Two) => "Q2o",
        (Rank::Jack, Rank::Ten) => "JTo",
        (Rank::Jack, Rank::Nine) => "J9o",
        (Rank::Jack, Rank::Eight) => "J8o",
        (Rank::Jack, Rank::Seven) => "J7o",
        (Rank::Jack, Rank::Six) => "J6o",
        (Rank::Jack, Rank::Five) => "J5o",
        (Rank::Jack, Rank::Four) => "J4o",
        (Rank::Jack, Rank::Three) => "J3o",
        (Rank::Jack, Rank::Two) => "J2o",
        (Rank::Ten, Rank::Nine) => "T9o",
        (Rank::Ten, Rank::Eight) => "T8o",
        (Rank::Ten, Rank::Seven) => "T7o",
        (Rank::Ten, Rank::Six) => "T6o",
        (Rank::Ten, Rank::Five) => "T5o",
        (Rank::Ten, Rank::Four) => "T4o",
        (Rank::Ten, Rank::Three) => "T3o",
        (Rank::Ten, Rank::Two) => "T2o",
        (Rank::Nine, Rank::Eight) => "98o",
        (Rank::Nine, Rank::Seven) => "97o",
        (Rank::Nine, Rank::Six) => "96o",
        (Rank::Nine, Rank::Five) => "95o",
        (Rank::Nine, Rank::Four) => "94o",
        (Rank::Nine, Rank::Three) => "93o",
        (Rank::Nine, Rank::Two) => "92o",
        (Rank::Eight, Rank::Seven) => "87o",
        (Rank::Eight, Rank::Six) => "86o",
        (Rank::Eight, Rank::Five) => "85o",
        (Rank::Eight, Rank::Four) => "84o",
        (Rank::Eight, Rank::Three) => "83o",
        (Rank::Eight, Rank::Two) => "82o",
        (Rank::Seven, Rank::Six) => "76o",
        (Rank::Seven, Rank::Five) => "75o",
        (Rank::Seven, Rank::Four) => "74o",
        (Rank::Seven, Rank::Three) => "73o",
        (Rank::Seven, Rank::Two) => "72o",
        (Rank::Six, Rank::Five) => "65o",
        (Rank::Six, Rank::Four) => "64o",
        (Rank::Six, Rank::Three) => "63o",
        (Rank::Six, Rank::Two) => "62o",
        (Rank::Five, Rank::Four) => "54o",
        (Rank::Five, Rank::Three) => "53o",
        (Rank::Five, Rank::Two) => "52o",
        (Rank::Four, Rank::Three) => "43o",
        (Rank::Four, Rank::Two) => "42o",
        (Rank::Three, Rank::Two) => "32o",
        _ => "XXo",
    }
}
