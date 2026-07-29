/* 52-Card Texas Hold'em Game Engine with 7-card hand evaluation and Card Abstraction. */

use rand::seq::SliceRandom;
use rand::Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rank {
    Two = 0, Three, Four, Five, Six, Seven, Eight, Nine, Ten,
    Jack, Queen, King, Ace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Suit {
    Clubs = 0, Diamonds, Hearts, Spades,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
}

impl Card {
    pub fn new(rank: Rank, suit: Suit) -> Self {
        Card { rank, suit }
    }

    pub fn to_string(self) -> String {
        let r = match self.rank {
            Rank::Two => "2", Rank::Three => "3", Rank::Four => "4", Rank::Five => "5",
            Rank::Six => "6", Rank::Seven => "7", Rank::Eight => "8", Rank::Nine => "9",
            Rank::Ten => "10", Rank::Jack => "J", Rank::Queen => "Q", Rank::King => "K",
            Rank::Ace => "A",
        };
        let s = match self.suit {
            Suit::Clubs => "♣", Suit::Diamonds => "♦", Suit::Hearts => "♥", Suit::Spades => "♠",
        };
        format!("'{}{}'", r, s)
    }
}

pub const ALL_52_CARDS: [Card; 52] = [
    Card{rank: Rank::Two, suit: Suit::Clubs}, Card{rank: Rank::Three, suit: Suit::Clubs},
    Card{rank: Rank::Four, suit: Suit::Clubs}, Card{rank: Rank::Five, suit: Suit::Clubs},
    Card{rank: Rank::Six, suit: Suit::Clubs}, Card{rank: Rank::Seven, suit: Suit::Clubs},
    Card{rank: Rank::Eight, suit: Suit::Clubs}, Card{rank: Rank::Nine, suit: Suit::Clubs},
    Card{rank: Rank::Ten, suit: Suit::Clubs}, Card{rank: Rank::Jack, suit: Suit::Clubs},
    Card{rank: Rank::Queen, suit: Suit::Clubs}, Card{rank: Rank::King, suit: Suit::Clubs},
    Card{rank: Rank::Ace, suit: Suit::Clubs},
    Card{rank: Rank::Two, suit: Suit::Diamonds}, Card{rank: Rank::Three, suit: Suit::Diamonds},
    Card{rank: Rank::Four, suit: Suit::Diamonds}, Card{rank: Rank::Five, suit: Suit::Diamonds},
    Card{rank: Rank::Six, suit: Suit::Diamonds}, Card{rank: Rank::Seven, suit: Suit::Diamonds},
    Card{rank: Rank::Eight, suit: Suit::Diamonds}, Card{rank: Rank::Nine, suit: Suit::Diamonds},
    Card{rank: Rank::Ten, suit: Suit::Diamonds}, Card{rank: Rank::Jack, suit: Suit::Diamonds},
    Card{rank: Rank::Queen, suit: Suit::Diamonds}, Card{rank: Rank::King, suit: Suit::Diamonds},
    Card{rank: Rank::Ace, suit: Suit::Diamonds},
    Card{rank: Rank::Two, suit: Suit::Hearts}, Card{rank: Rank::Three, suit: Suit::Hearts},
    Card{rank: Rank::Four, suit: Suit::Hearts}, Card{rank: Rank::Five, suit: Suit::Hearts},
    Card{rank: Rank::Six, suit: Suit::Hearts}, Card{rank: Rank::Seven, suit: Suit::Hearts},
    Card{rank: Rank::Eight, suit: Suit::Hearts}, Card{rank: Rank::Nine, suit: Suit::Hearts},
    Card{rank: Rank::Ten, suit: Suit::Hearts}, Card{rank: Rank::Jack, suit: Suit::Hearts},
    Card{rank: Rank::Queen, suit: Suit::Hearts}, Card{rank: Rank::King, suit: Suit::Hearts},
    Card{rank: Rank::Ace, suit: Suit::Hearts},
    Card{rank: Rank::Two, suit: Suit::Spades}, Card{rank: Rank::Three, suit: Suit::Spades},
    Card{rank: Rank::Four, suit: Suit::Spades}, Card{rank: Rank::Five, suit: Suit::Spades},
    Card{rank: Rank::Six, suit: Suit::Spades}, Card{rank: Rank::Seven, suit: Suit::Spades},
    Card{rank: Rank::Eight, suit: Suit::Spades}, Card{rank: Rank::Nine, suit: Suit::Spades},
    Card{rank: Rank::Ten, suit: Suit::Spades}, Card{rank: Rank::Jack, suit: Suit::Spades},
    Card{rank: Rank::Queen, suit: Suit::Spades}, Card{rank: Rank::King, suit: Suit::Spades},
    Card{rank: Rank::Ace, suit: Suit::Spades},
];

pub const FOLD: u8 = 0;
pub const CALL_CHECK: u8 = 1;
pub const RAISE_MIN: u8 = 2;
pub const RAISE_HALF_POT: u8 = 3;

#[derive(Debug, Clone)]
pub struct TexasHoldemGame {
    pub hole: [[Card; 2]; 2],
    pub board: Vec<Card>,
    pub deck_remaining: Vec<Card>,
    pub current_player: usize,
    pub round: u8,
    pub contributions: [i32; 2],
    pub history: [Vec<u8>; 4],
    pub terminal: bool,
    pub returns: [f64; 2],
}

impl TexasHoldemGame {
    pub fn new_random<R: Rng>(rng: &mut R) -> Self {
        let mut deck = ALL_52_CARDS.to_vec();
        deck.shuffle(rng);

        let hole0 = [deck[0], deck[1]];
        let hole1 = [deck[2], deck[3]];
        let deck_rem = deck[4..].to_vec();

        TexasHoldemGame {
            hole: [hole0, hole1],
            board: Vec::new(),
            deck_remaining: deck_rem,
            current_player: 0,
            round: 1,
            contributions: [10, 20],
            history: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            terminal: false,
            returns: [0.0, 0.0],
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal
    }

    pub fn get_returns(&self) -> [f64; 2] {
        self.returns
    }

    pub fn current_player(&self) -> usize {
        self.current_player
    }

    pub fn legal_actions(&self) -> Vec<u8> {
        if self.terminal { return vec![]; }
        let opp = 1 - self.current_player;
        let diff = self.contributions[opp] - self.contributions[self.current_player];
        let my_contrib = self.contributions[self.current_player];

        if my_contrib >= 1000 || self.contributions[opp] >= 1000 {
            if diff > 0 { vec![FOLD, CALL_CHECK] } else { vec![CALL_CHECK] }
        } else if diff > 0 {
            vec![FOLD, CALL_CHECK, RAISE_MIN, RAISE_HALF_POT]
        } else {
            vec![CALL_CHECK, RAISE_MIN, RAISE_HALF_POT]
        }
    }

    pub fn apply_action(&self, action: u8) -> TexasHoldemGame {
        let mut next = self.clone();
        let r_idx = (next.round - 1) as usize;
        next.history[r_idx].push(action);

        let opp = 1 - next.current_player;
        let pot = next.contributions[0] + next.contributions[1];

        let stack_limit = 1000i32;

        match action {
            FOLD => {
                let winner = opp;
                let loser = next.current_player;
                next.returns[winner] = next.contributions[loser] as f64;
                next.returns[loser] = -(next.contributions[loser] as f64);
                next.terminal = true;
                return next;
            }
            CALL_CHECK => {
                let diff = (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                next.contributions[next.current_player] = (next.contributions[next.current_player] + diff).min(stack_limit);
            }
            RAISE_MIN => {
                let diff = (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                let raise_amt = 40;
                next.contributions[next.current_player] = (next.contributions[next.current_player] + diff + raise_amt).min(stack_limit);
            }
            RAISE_HALF_POT => {
                let diff = (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                let raise_amt = (pot / 2).max(40);
                next.contributions[next.current_player] = (next.contributions[next.current_player] + diff + raise_amt).min(stack_limit);
            }
            _ => unreachable!(),
        }

        // Check round transition
        let hist = &next.history[r_idx];
        let n = hist.len();
        let round_over = n >= 2 && {
            let last = hist[n - 1];
            let prev = hist[n - 2];
            (last == CALL_CHECK && prev == CALL_CHECK) || (last == CALL_CHECK && (prev == RAISE_MIN || prev == RAISE_HALF_POT))
        };

        if round_over {
            if next.round == 1 {
                // Deal Flop (3 cards)
                next.round = 2;
                next.board.push(next.deck_remaining.remove(0));
                next.board.push(next.deck_remaining.remove(0));
                next.board.push(next.deck_remaining.remove(0));
                next.current_player = 0;
            } else if next.round == 2 {
                // Deal Turn (1 card)
                next.round = 3;
                next.board.push(next.deck_remaining.remove(0));
                next.current_player = 0;
            } else if next.round == 3 {
                // Deal River (1 card)
                next.round = 4;
                next.board.push(next.deck_remaining.remove(0));
                next.current_player = 0;
            } else {
                // Showdown
                next.resolve_showdown();
            }
        } else {
            next.current_player = opp;
        }

        next
    }

    fn resolve_showdown(&mut self) {
        let score0 = evaluate_7cards(&self.hole[0], &self.board);
        let score1 = evaluate_7cards(&self.hole[1], &self.board);

        let pot = self.contributions[0] + self.contributions[1];
        if score0 > score1 {
            self.returns[0] = (pot - self.contributions[0]) as f64;
            self.returns[1] = -(self.contributions[1] as f64);
        } else if score1 > score0 {
            self.returns[0] = -(self.contributions[0] as f64);
            self.returns[1] = (pot - self.contributions[1]) as f64;
        } else {
            self.returns[0] = 0.0;
            self.returns[1] = 0.0;
        }
        self.terminal = true;
    }
}

/* Fast 7-Card Poker Hand Evaluator (Bitwise score calculation) */
pub fn evaluate_7cards(hole: &[Card; 2], board: &[Card]) -> u64 {
    let mut all_cards = Vec::with_capacity(7);
    all_cards.extend_from_slice(hole);
    all_cards.extend_from_slice(board);

    /*  Sort by rank descending */
    all_cards.sort_by(|a, b| b.rank.cmp(&a.rank));

    let mut rank_counts = [0u8; 13];
    let mut suit_counts = [0u8; 4];
    for c in &all_cards {
        rank_counts[c.rank as usize] += 1;
        suit_counts[c.suit as usize] += 1;
    }

    /*  Check Flush */
    let flush_suit = suit_counts.iter().position(|&cnt| cnt >= 5);

    /*  Check Straight  */
    let mut straight_high = None;
    let mut consecutive = 0;
    for r in (0..13).rev() {
        if rank_counts[r] > 0 {
            consecutive += 1;
            if consecutive >= 5 {
                straight_high = Some(r as u64 + 4);
                break;
            }
        } else {
            consecutive = 0;
        }
    }
    /*  Ace-low straight A-2-3-4-5  */
    if straight_high.is_none() && rank_counts[12] > 0 && rank_counts[0] > 0 && rank_counts[1] > 0 && rank_counts[2] > 0 && rank_counts[3] > 0 {
        straight_high = Some(3);
    }

    let mut pairs = Vec::new();
    let mut trips = Vec::new();
    let mut quads = Vec::new();

    for r in (0..13).rev() {
        match rank_counts[r] {
            4 => quads.push(r as u64),
            3 => trips.push(r as u64),
            2 => pairs.push(r as u64),
            _ => {}
        }
    }

    /*  
     * Category scores:  
     * 8: Straight Flush, 7: Quads, 6: Full House, 
     * 5: Flush, 4: Straight, 3: Trips, 
     * 2: Two Pair, 1: Pair, 0: High Card  
     */
    if let (Some(_f_suit), Some(st_h)) = (flush_suit, straight_high) {
        return (8 << 32) | st_h;
    }
    if let Some(&q) = quads.first() {
        let kicker = (0..13).rev().find(|&r| r as u64 != q && rank_counts[r] > 0).unwrap_or(0) as u64;
        return (7 << 32) | (q << 16) | kicker;
    }
    if !trips.is_empty() && (!pairs.is_empty() || trips.len() > 1) {
        let t = trips[0];
        let p = if trips.len() > 1 { trips[1] } else { pairs[0] };
        return (6 << 32) | (t << 16) | p;
    }
    if let Some(f_suit) = flush_suit {
        let flush_cards: Vec<u64> = all_cards.iter().filter(|c| c.suit as usize == f_suit).take(5).map(|c| c.rank as u64).collect();
        let mut score = 5u64 << 32;
        for (i, &r) in flush_cards.iter().enumerate() {
            score |= r << (16 - i * 4);
        }
        return score;
    }
    if let Some(st_h) = straight_high {
        return (4 << 32) | st_h;
    }
    if let Some(&t) = trips.first() {
        let kickers: Vec<u64> = (0..13).rev().filter(|&r| r as u64 != t && rank_counts[r] > 0).take(2).map(|r| r as u64).collect();
        return (3 << 32) | (t << 16) | (kickers.get(0).copied().unwrap_or(0) << 8) | kickers.get(1).copied().unwrap_or(0);
    }
    if pairs.len() >= 2 {
        let p1 = pairs[0];
        let p2 = pairs[1];
        let kicker = (0..13).rev().find(|&r| r as u64 != p1 && r as u64 != p2 && rank_counts[r] > 0).unwrap_or(0) as u64;
        return (2 << 32) | (p1 << 16) | (p2 << 8) | kicker;
    }
    if let Some(&p) = pairs.first() {
        let kickers: Vec<u64> = (0..13).rev().filter(|&r| r as u64 != p && rank_counts[r] > 0).take(3).map(|r| r as u64).collect();
        return (1 << 32) | (p << 16) | (kickers.get(0).copied().unwrap_or(0) << 8) | kickers.get(1).copied().unwrap_or(0);
    }

    let top5: Vec<u64> = (0..13).rev().filter(|&r| rank_counts[r] > 0).take(5).map(|r| r as u64).collect();
    let mut score = 0u64;
    for (i, &r) in top5.iter().enumerate() {
        score |= r << (16 - i * 4);
    }
    score
}
