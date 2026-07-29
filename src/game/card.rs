/* Leduc Poker uses a 6-card deck: J, Q, K in two suits. */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rank {
    Jack = 0,
    Queen = 1,
    King = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Suit {
    Club = 0,
    Diamond = 1,
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

    /* Returns a 0-based index (0..5) for this card. */
    pub fn index(self) -> usize {
        self.rank as usize * 2 + self.suit as usize
    }

    pub fn name(self) -> &'static str {
        match (self.rank, self.suit) {
            (Rank::Jack, Suit::Club) => "Jc",
            (Rank::Jack, Suit::Diamond) => "Jd",
            (Rank::Queen, Suit::Club) => "Qc",
            (Rank::Queen, Suit::Diamond) => "Qd",
            (Rank::King, Suit::Club) => "Kc",
            (Rank::King, Suit::Diamond) => "Kd",
        }
    }
}

/* Full 6-card Leduc deck in a fixed order. */
pub const ALL_CARDS: [Card; 6] = [
    Card {
        rank: Rank::Jack,
        suit: Suit::Club,
    },
    Card {
        rank: Rank::Jack,
        suit: Suit::Diamond,
    },
    Card {
        rank: Rank::Queen,
        suit: Suit::Club,
    },
    Card {
        rank: Rank::Queen,
        suit: Suit::Diamond,
    },
    Card {
        rank: Rank::King,
        suit: Suit::Club,
    },
    Card {
        rank: Rank::King,
        suit: Suit::Diamond,
    },
];
