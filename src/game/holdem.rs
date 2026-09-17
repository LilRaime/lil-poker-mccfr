/* 52-Card Texas Hold'em Game Engine with 7-card hand evaluation and Card Abstraction. */

use rand::Rng;

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

pub const ALL_52_CARDS: [Card; 52] = [
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

pub const FOLD: u8 = 0;
pub const CALL_CHECK: u8 = 1;
pub const RAISE_MIN: u8 = 2;
pub const RAISE_HALF_POT: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Board {
    cards: [Card; 5],
    len: u8,
}

impl Board {
    #[inline(always)]
    pub fn new() -> Self {
        Board {
            cards: [ALL_52_CARDS[0]; 5],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn from_slice(cards: &[Card]) -> Self {
        let mut b = Self::new();
        for &c in cards.iter().take(5) {
            b.push(c);
        }
        b
    }

    #[inline(always)]
    pub fn push(&mut self, card: Card) {
        if (self.len as usize) < 5 {
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
    actions: [u8; 32],
    len: u8,
}

impl RoundHistory {
    #[inline(always)]
    pub fn new() -> Self {
        RoundHistory {
            actions: [0u8; 32],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn from_slice(actions: &[u8]) -> Self {
        let mut rh = Self::new();
        for &a in actions.iter().take(32) {
            rh.push(a);
        }
        rh
    }

    #[inline(always)]
    pub fn push(&mut self, action: u8) {
        if (self.len as usize) < 32 {
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
    cards: [Card; 52],
    ptr: u8,
}

impl DeckRemaining {
    #[inline(always)]
    pub fn from_slice(cards: &[Card]) -> Self {
        let mut arr = [ALL_52_CARDS[0]; 52];
        let n = cards.len().min(52);
        arr[..n].copy_from_slice(&cards[..n]);
        DeckRemaining { cards: arr, ptr: 0 }
    }

    #[inline(always)]
    pub fn deal_one(&mut self) -> Card {
        let c = self.cards[self.ptr as usize];
        self.ptr += 1;
        c
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        (52 - self.ptr) as usize
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.ptr >= 52
    }
}

impl Default for DeckRemaining {
    fn default() -> Self {
        DeckRemaining {
            cards: ALL_52_CARDS,
            ptr: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TexasHoldemGame {
    pub hole: [[Card; 2]; 2],
    pub board: Board,
    pub deck_remaining: DeckRemaining,
    pub current_player: usize,
    pub round: u8,
    pub raises_this_round: u8,
    pub contributions: [i32; 2],
    pub history: [RoundHistory; 4],
    pub terminal: bool,
    pub returns: [f64; 2],
}

impl TexasHoldemGame {
    pub fn new_random<R: Rng>(rng: &mut R) -> Self {
        let mut deck = ALL_52_CARDS;
        /* Partial Fisher-Yates shuffle: only 9 cards needed per episode */
        for i in 0..9 {
            let j = rng.gen_range(i..52);
            deck.swap(i, j);
        }

        let hole0 = [deck[0], deck[1]];
        let hole1 = [deck[2], deck[3]];
        let deck_rem = DeckRemaining::from_slice(&deck[4..9]);

        TexasHoldemGame {
            hole: [hole0, hole1],
            board: Board::new(),
            deck_remaining: deck_rem,
            current_player: 0,
            round: 1,
            raises_this_round: 0,
            contributions: [10, 20],
            history: [RoundHistory::new(); 4],
            terminal: false,
            returns: [0.0, 0.0],
        }
    }

    #[inline(always)]
    pub fn is_terminal(&self) -> bool {
        self.terminal
    }

    #[inline(always)]
    pub fn get_returns(&self) -> [f64; 2] {
        self.returns
    }

    #[inline(always)]
    pub fn current_player(&self) -> usize {
        self.current_player
    }

    #[inline(always)]
    pub fn legal_actions_buf(&self, out: &mut [u8; 4]) -> usize {
        if self.terminal {
            return 0;
        }
        let opp = 1 - self.current_player;
        let diff = self.contributions[opp] - self.contributions[self.current_player];
        let my_contrib = self.contributions[self.current_player];
        let raise_capped = self.raises_this_round >= 3;

        if my_contrib >= 1000 || self.contributions[opp] >= 1000 || raise_capped {
            if diff > 0 {
                out[0] = FOLD;
                out[1] = CALL_CHECK;
                2
            } else {
                out[0] = CALL_CHECK;
                1
            }
        } else if diff > 0 {
            out[0] = FOLD;
            out[1] = CALL_CHECK;
            out[2] = RAISE_MIN;
            out[3] = RAISE_HALF_POT;
            4
        } else {
            out[0] = CALL_CHECK;
            out[1] = RAISE_MIN;
            out[2] = RAISE_HALF_POT;
            3
        }
    }

    pub fn legal_actions(&self) -> Vec<u8> {
        let mut buf = [0u8; 4];
        let n = self.legal_actions_buf(&mut buf);
        buf[..n].to_vec()
    }

    #[inline(always)]
    pub fn apply_action(&self, action: u8) -> TexasHoldemGame {
        let mut next = *self;
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
                let diff =
                    (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                next.contributions[next.current_player] =
                    (next.contributions[next.current_player] + diff).min(stack_limit);
            }
            RAISE_MIN => {
                let diff =
                    (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                let raise_amt = 40;
                next.contributions[next.current_player] =
                    (next.contributions[next.current_player] + diff + raise_amt).min(stack_limit);
                next.raises_this_round += 1;
            }
            RAISE_HALF_POT => {
                let diff =
                    (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                let raise_amt = (pot / 2).max(40);
                next.contributions[next.current_player] =
                    (next.contributions[next.current_player] + diff + raise_amt).min(stack_limit);
                next.raises_this_round += 1;
            }
            _ => unreachable!(),
        }

        let hist = &next.history[r_idx];
        let n = hist.len();
        let round_over = n >= 2 && {
            let last = hist[n - 1];
            let prev = hist[n - 2];
            (last == CALL_CHECK && prev == CALL_CHECK)
                || (last == CALL_CHECK && (prev == RAISE_MIN || prev == RAISE_HALF_POT))
        };

        if round_over {
            next.raises_this_round = 0;
            if next.round == 1 {
                /* Deal Flop (3 cards) */
                next.round = 2;
                next.board.push(next.deck_remaining.deal_one());
                next.board.push(next.deck_remaining.deal_one());
                next.board.push(next.deck_remaining.deal_one());
                next.current_player = 0;
            } else if next.round == 2 {
                /* Deal Turn (1 card) */
                next.round = 3;
                next.board.push(next.deck_remaining.deal_one());
                next.current_player = 0;
            } else if next.round == 3 {
                /* Deal River (1 card) */
                next.round = 4;
                next.board.push(next.deck_remaining.deal_one());
                next.current_player = 0;
            } else {
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

/* Fast 7-Card Poker Hand Evaluator (Bitwise score calculation, 100% stack-allocated) */
#[inline]
pub fn evaluate_7cards(hole: &[Card; 2], board: &[Card]) -> u64 {
    let mut all_cards = [ALL_52_CARDS[0]; 7];
    all_cards[0] = hole[0];
    all_cards[1] = hole[1];
    let board_n = board.len().min(5);
    all_cards[2..(board_n + 2)].copy_from_slice(&board[..board_n]);
    let total_cards = 2 + board_n;
    let cards = &mut all_cards[..total_cards];

    /* Sort by rank descending */
    cards.sort_by_key(|b| std::cmp::Reverse(b.rank));

    let mut rank_counts = [0u8; 13];
    let mut suit_counts = [0u8; 4];
    for c in cards.iter() {
        rank_counts[c.rank as usize] += 1;
        suit_counts[c.suit as usize] += 1;
    }

    /* Check Flush */
    let flush_suit = suit_counts.iter().position(|&cnt| cnt >= 5);

    /* Check Straight */
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
    /* Ace-low straight A-2-3-4-5 */
    if straight_high.is_none()
        && rank_counts[12] > 0
        && rank_counts[0] > 0
        && rank_counts[1] > 0
        && rank_counts[2] > 0
        && rank_counts[3] > 0
    {
        straight_high = Some(3);
    }

    let mut quads = [0u64; 1];
    let mut quads_len = 0;
    let mut trips = [0u64; 2];
    let mut trips_len = 0;
    let mut pairs = [0u64; 3];
    let mut pairs_len = 0;

    for r in (0..13).rev() {
        match rank_counts[r] {
            4 if quads_len < 1 => {
                quads[quads_len] = r as u64;
                quads_len += 1;
            }
            3 if trips_len < 2 => {
                trips[trips_len] = r as u64;
                trips_len += 1;
            }
            2 if pairs_len < 3 => {
                pairs[pairs_len] = r as u64;
                pairs_len += 1;
            }
            _ => {}
        }
    }

    /* Check Straight Flush */
    if let Some(f_suit) = flush_suit {
        let mut flush_ranks = [0u8; 7];
        let mut f_count = 0;
        for c in cards.iter() {
            if c.suit as usize == f_suit {
                flush_ranks[f_count] = c.rank as u8;
                f_count += 1;
            }
        }

        let mut sf_high = None;
        let mut f_consec = 1;
        for i in 0..f_count.saturating_sub(1) {
            if flush_ranks[i] == flush_ranks[i + 1] + 1 {
                f_consec += 1;
                if f_consec >= 5 && sf_high.is_none() {
                    sf_high = Some(flush_ranks[i + 1] as u64 + 4);
                }
            } else if flush_ranks[i] != flush_ranks[i + 1] {
                f_consec = 1;
            }
        }
        if sf_high.is_none()
            && flush_ranks[..f_count].contains(&12)
            && flush_ranks[..f_count].contains(&0)
            && flush_ranks[..f_count].contains(&1)
            && flush_ranks[..f_count].contains(&2)
            && flush_ranks[..f_count].contains(&3)
        {
            sf_high = Some(3);
        }

        if let Some(st_h) = sf_high {
            return (8 << 32) | st_h;
        }
    }

    if quads_len > 0 {
        let q = quads[0];
        let kicker = (0..13)
            .rev()
            .find(|&r| r as u64 != q && rank_counts[r] > 0)
            .unwrap_or(0) as u64;
        return (7 << 32) | (q << 16) | kicker;
    }

    if trips_len > 0 && (pairs_len > 0 || trips_len > 1) {
        let t = trips[0];
        let p = if trips_len > 1 { trips[1] } else { pairs[0] };
        return (6 << 32) | (t << 16) | p;
    }

    if let Some(f_suit) = flush_suit {
        let mut score = 5u64 << 32;
        let mut count = 0;
        for c in cards.iter() {
            if c.suit as usize == f_suit {
                score |= (c.rank as u64) << (16 - count * 4);
                count += 1;
                if count == 5 {
                    break;
                }
            }
        }
        return score;
    }

    if let Some(st_h) = straight_high {
        return (4 << 32) | st_h;
    }

    if trips_len > 0 {
        let t = trips[0];
        let mut kickers = [0u64; 2];
        let mut k_count = 0;
        for r in (0..13).rev() {
            if r as u64 != t && rank_counts[r] > 0 {
                kickers[k_count] = r as u64;
                k_count += 1;
                if k_count == 2 {
                    break;
                }
            }
        }
        return (3 << 32) | (t << 16) | (kickers[0] << 8) | kickers[1];
    }

    if pairs_len >= 2 {
        let p1 = pairs[0];
        let p2 = pairs[1];
        let kicker = (0..13)
            .rev()
            .find(|&r| r as u64 != p1 && r as u64 != p2 && rank_counts[r] > 0)
            .unwrap_or(0) as u64;
        return (2 << 32) | (p1 << 16) | (p2 << 8) | kicker;
    }

    if pairs_len >= 1 {
        let p = pairs[0];
        let mut kickers = [0u64; 3];
        let mut k_count = 0;
        for r in (0..13).rev() {
            if r as u64 != p && rank_counts[r] > 0 {
                kickers[k_count] = r as u64;
                k_count += 1;
                if k_count == 3 {
                    break;
                }
            }
        }
        return (1 << 32) | (p << 16) | (kickers[0] << 8) | kickers[1];
    }

    let mut score = 0u64;
    let mut count = 0;
    for r in (0..13).rev() {
        if rank_counts[r] > 0 {
            score |= (r as u64) << (16 - count * 4);
            count += 1;
            if count == 5 {
                break;
            }
        }
    }
    score
}
