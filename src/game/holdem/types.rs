/*
 * Core Card, Rank, Suit, Board, and History types for Texas Hold'em.
 */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rank {
    Two = 0,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King,
    Ace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Suit {
    Clubs = 0,
    Diamonds,
    Hearts,
    Spades,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
}

impl Default for Card {
    fn default() -> Self {
        Card {
            rank: Rank::Two,
            suit: Suit::Clubs,
        }
    }
}

impl Card {
    pub fn new(rank: Rank, suit: Suit) -> Self {
        Card { rank, suit }
    }
}

impl std::fmt::Display for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let r = match self.rank {
            Rank::Two => "2",
            Rank::Three => "3",
            Rank::Four => "4",
            Rank::Five => "5",
            Rank::Six => "6",
            Rank::Seven => "7",
            Rank::Eight => "8",
            Rank::Nine => "9",
            Rank::Ten => "10",
            Rank::Jack => "J",
            Rank::Queen => "Q",
            Rank::King => "K",
            Rank::Ace => "A",
        };
        let s = match self.suit {
            Suit::Clubs => "♣",
            Suit::Diamonds => "♦",
            Suit::Hearts => "♥",
            Suit::Spades => "♠",
        };
        write!(f, "'{}{}'", r, s)
    }
}

use crate::game::config::{MAX_ROUND_ACTIONS, NUM_CARDS_DECK, NUM_COMMUNITY_CARDS};

pub const ALL_52_CARDS: [Card; NUM_CARDS_DECK] = [
    Card {
        rank: Rank::Two,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Three,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Four,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Five,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Six,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Seven,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Eight,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Nine,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Ten,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Jack,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Queen,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::King,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Ace,
        suit: Suit::Clubs,
    },
    Card {
        rank: Rank::Two,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Three,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Four,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Five,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Six,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Seven,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Eight,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Nine,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Ten,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Jack,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Queen,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::King,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Ace,
        suit: Suit::Diamonds,
    },
    Card {
        rank: Rank::Two,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Three,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Four,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Five,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Six,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Seven,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Eight,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Nine,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Ten,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Jack,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Queen,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::King,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Ace,
        suit: Suit::Hearts,
    },
    Card {
        rank: Rank::Two,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Three,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Four,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Five,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Six,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Seven,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Eight,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Nine,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Ten,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Jack,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Queen,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::King,
        suit: Suit::Spades,
    },
    Card {
        rank: Rank::Ace,
        suit: Suit::Spades,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Board {
    cards: [Card; NUM_COMMUNITY_CARDS],
    len: u8,
}

impl Board {
    #[inline(always)]
    pub fn new() -> Self {
        Board {
            cards: [ALL_52_CARDS[0]; NUM_COMMUNITY_CARDS],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn from_slice(cards: &[Card]) -> Self {
        let mut b = Self::new();
        for &c in cards.iter().take(NUM_COMMUNITY_CARDS) {
            b.push(c);
        }
        b
    }

    #[inline(always)]
    pub fn push(&mut self, card: Card) {
        if (self.len as usize) < NUM_COMMUNITY_CARDS {
            self.cards[self.len as usize] = card;
            self.len += 1;
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[Card] {
        &self.cards[..self.len as usize]
    }
}

impl std::ops::Deref for Board {
    type Target = [Card];
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'a> IntoIterator for &'a Board {
    type Item = &'a Card;
    type IntoIter = std::slice::Iter<'a, Card>;
    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RoundHistory {
    actions: [u8; MAX_ROUND_ACTIONS],
    len: u8,
}

impl RoundHistory {
    #[inline(always)]
    pub fn new() -> Self {
        RoundHistory {
            actions: [0u8; MAX_ROUND_ACTIONS],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn from_slice(actions: &[u8]) -> Self {
        let mut rh = Self::new();
        for &a in actions.iter().take(MAX_ROUND_ACTIONS) {
            rh.push(a);
        }
        rh
    }

    #[inline(always)]
    pub fn push(&mut self, action: u8) {
        if (self.len as usize) < MAX_ROUND_ACTIONS {
            self.actions[self.len as usize] = action;
            self.len += 1;
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        &self.actions[..self.len as usize]
    }
}

impl std::ops::Deref for RoundHistory {
    type Target = [u8];
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'a> IntoIterator for &'a RoundHistory {
    type Item = &'a u8;
    type IntoIter = std::slice::Iter<'a, u8>;
    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeckRemaining {
    cards: [Card; NUM_COMMUNITY_CARDS],
    ptr: u8,
}

impl DeckRemaining {
    #[inline(always)]
    pub fn from_slice(cards: &[Card]) -> Self {
        let mut arr = [ALL_52_CARDS[0]; NUM_COMMUNITY_CARDS];
        let n = cards.len().min(NUM_COMMUNITY_CARDS);
        arr[..n].copy_from_slice(&cards[..n]);
        DeckRemaining { cards: arr, ptr: 0 }
    }

    #[inline(always)]
    pub fn peek5(&self) -> &[Card; NUM_COMMUNITY_CARDS] {
        &self.cards
    }

    #[inline(always)]
    pub fn deal_one(&mut self) -> Card {
        let c = self.cards[self.ptr as usize];
        self.ptr += 1;
        c
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        NUM_COMMUNITY_CARDS.saturating_sub(self.ptr as usize)
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.ptr as usize >= NUM_COMMUNITY_CARDS
    }
}

impl Default for DeckRemaining {
    fn default() -> Self {
        let mut arr = [ALL_52_CARDS[0]; NUM_COMMUNITY_CARDS];
        arr.copy_from_slice(&ALL_52_CARDS[..NUM_COMMUNITY_CARDS]);
        DeckRemaining { cards: arr, ptr: 0 }
    }
}
