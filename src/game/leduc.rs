/*
 * Leduc Poker game implementation (2 players, 6-card deck).
 * Actions: 0 = Fold, 1 = Call/Check, 2 = Raise.
 */
use crate::game::card::{Card, ALL_CARDS};
use rand::seq::SliceRandom;

pub const FOLD: u8 = 0;
pub const CALL: u8 = 1;
pub const RAISE: u8 = 2;

pub const NUM_ACTIONS: usize = 3;

/* Compact round-history string used as part of the infoset key. */
fn action_char(a: u8) -> char {
    match a {
        FOLD => 'f',
        CALL => 'c',
        RAISE => 'r',
        _ => '?',
    }
}

#[derive(Clone)]
pub struct LeducGame {
    pub hole: [Card; 2],
    board_card: Card,
    pub board: Option<Card>,
    pub current_player: usize,
    pub round: u8,
    pub raises_this_round: u8,
    pub contributions: [i32; 2],
    pub history: [Vec<u8>; 2],
    pub terminal: bool,
    pub returns: [f64; 2],
}

impl LeducGame {
    /* Create a new shuffled game. Deals hole cards AND community card upfront. */
    pub fn new_random<R: rand::Rng>(rng: &mut R) -> Self {
        let mut deck: Vec<Card> = ALL_CARDS.to_vec();
        deck.shuffle(rng);
        LeducGame::new_with_cards(deck[0], deck[1], deck[2])
    }

    /* Create a game with a specific card deal (for exploitability enumeration). */
    pub fn new_with_cards(hole0: Card, hole1: Card, board: Card) -> Self {
        LeducGame {
            hole: [hole0, hole1],
            board_card: board,
            board: None,
            current_player: 0,
            round: 1,
            raises_this_round: 0,
            contributions: [1, 1],
            history: [Vec::new(), Vec::new()],
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

    /* Legal actions at the current node. */
    pub fn legal_actions(&self) -> Vec<u8> {
        if self.terminal {
            return vec![];
        }
        let round_idx = (self.round - 1) as usize;
        let history = &self.history[round_idx];
        let last = history.last().copied();

        match last {
            Some(RAISE) => {
                if self.raises_this_round < 2 {
                    vec![FOLD, CALL, RAISE]
                } else {
                    vec![FOLD, CALL]
                }
            }
            _ => vec![CALL, RAISE],
        }
    }

    /* Information set key for player */
    pub fn infoset_key(&self, player: usize) -> String {
        let card = self.hole[player].name();
        let board = match self.board {
            Some(c) => c.name().to_string(),
            None => String::from("_"),
        };
        let h1: String = self.history[0].iter().map(|&a| action_char(a)).collect();
        let h2: String = self.history[1].iter().map(|&a| action_char(a)).collect();
        format!("{}/{}/{}/{}", card, board, h1, h2)
    }

    /* Apply action and return resulting game state */
    pub fn apply_action(&self, action: u8) -> LeducGame {
        let mut next = self.clone();
        let round_idx = (next.round - 1) as usize;
        next.history[round_idx].push(action);

        match action {
            FOLD => {
                let pot = next.contributions[0] + next.contributions[1];
                let winner = 1 - next.current_player;
                next.returns[winner] = (pot - next.contributions[winner]) as f64;
                next.returns[next.current_player] =
                    -(next.contributions[next.current_player]) as f64;
                next.terminal = true;
            }

            CALL => {
                let opp = 1 - next.current_player;
                let diff =
                    (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                next.contributions[next.current_player] += diff;

                next = next.advance_round_if_done();
            }

            RAISE => {
                let bet = if next.round == 1 { 2 } else { 4 };
                let opp = 1 - next.current_player;
                let diff =
                    (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                next.contributions[next.current_player] += diff + bet;
                next.raises_this_round += 1;
                next.current_player = 1 - next.current_player;
            }

            _ => unreachable!(),
        }

        next
    }

    /* Advance round if check-check or raise-call occurs */
    fn advance_round_if_done(mut self) -> LeducGame {
        let round_idx = (self.round - 1) as usize;
        let history = &self.history[round_idx];

        let round_over = {
            let n = history.len();
            if n < 2 {
                false
            } else {
                let last = history[n - 1];
                let prev = history[n - 2];
                (last == CALL && prev == CALL) || (last == CALL && prev == RAISE)
            }
        };

        if !round_over {
            self.current_player = 1 - self.current_player;
            return self;
        }

        if self.round == 1 {
            self.board = Some(self.board_card);
            self.round = 2;
            self.raises_this_round = 0;
            self.current_player = 0;
        } else {
            self.resolve_showdown();
        }

        self
    }

    fn resolve_showdown(&mut self) {
        let pot = self.contributions[0] + self.contributions[1];
        let board_rank = self.board.unwrap().rank;

        let rank0 = self.hole[0].rank;
        let rank1 = self.hole[1].rank;

        let pair0 = rank0 == board_rank;
        let pair1 = rank1 == board_rank;

        let winner: Option<usize> = match (pair0, pair1) {
            (true, false) => Some(0),
            (false, true) => Some(1),
            (true, true) => None,
            (false, false) => {
                if (rank0 as u8) > (rank1 as u8) {
                    Some(0)
                } else if (rank1 as u8) > (rank0 as u8) {
                    Some(1)
                } else {
                    None
                }
            }
        };

        match winner {
            Some(w) => {
                let loser = 1 - w;
                self.returns[w] = (pot - self.contributions[w]) as f64;
                self.returns[loser] = -(self.contributions[loser]) as f64;
            }
            None => {
                self.returns[0] = 0.0;
                self.returns[1] = 0.0;
            }
        }
        self.terminal = true;
    }
}
