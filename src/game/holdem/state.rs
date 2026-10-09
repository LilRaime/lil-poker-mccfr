/*
 * TexasHoldemGame state machine, transitions, betting logic, and payoffs.
 */

use crate::game::config::*;
use rand::Rng;

use super::actions::*;
use super::eval::evaluate_7cards;
use super::types::{Board, Card, DeckRemaining, RoundHistory, ALL_52_CARDS};

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
            contributions: [SMALL_BLIND, BIG_BLIND],
            history: [RoundHistory::new(); 4],
            terminal: false,
            returns: [0.0, 0.0],
        }
    }

    pub fn new_dealt(hole0: [Card; 2], hole1: [Card; 2], runout: &[Card]) -> Self {
        TexasHoldemGame {
            hole: [hole0, hole1],
            board: Board::new(),
            deck_remaining: DeckRemaining::from_slice(runout),
            current_player: 0,
            round: 1,
            raises_this_round: 0,
            contributions: [SMALL_BLIND, BIG_BLIND],
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
    pub fn legal_actions_buf(&self, out: &mut [u8; 6]) -> usize {
        if self.terminal {
            return 0;
        }
        let opp = 1 - self.current_player;
        let diff = self.contributions[opp] - self.contributions[self.current_player];
        let my_contrib = self.contributions[self.current_player];
        let raise_capped = self.raises_this_round >= MAX_RAISES_PER_ROUND;

        if my_contrib >= STACK_SIZE || self.contributions[opp] >= STACK_SIZE || raise_capped {
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
            out[3] = RAISE_THIRD_POT;
            out[4] = RAISE_HALF_POT;
            out[5] = ALL_IN;
            6
        } else {
            out[0] = CALL_CHECK;
            out[1] = RAISE_MIN;
            out[2] = RAISE_THIRD_POT;
            out[3] = RAISE_HALF_POT;
            out[4] = ALL_IN;
            5
        }
    }

    pub fn legal_actions(&self) -> Vec<u8> {
        let mut buf = [0u8; 6];
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
                    (next.contributions[next.current_player] + diff).min(STACK_SIZE);
            }
            RAISE_MIN => {
                let diff =
                    (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                let raise_amt = RAISE_MIN_AMT;
                next.contributions[next.current_player] =
                    (next.contributions[next.current_player] + diff + raise_amt).min(STACK_SIZE);
                next.raises_this_round += 1;
            }
            RAISE_THIRD_POT => {
                let diff =
                    (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                let raise_amt = (pot / 3).max(RAISE_THIRD_POT_FLOOR);
                next.contributions[next.current_player] =
                    (next.contributions[next.current_player] + diff + raise_amt).min(STACK_SIZE);
                next.raises_this_round += 1;
            }
            RAISE_HALF_POT => {
                let diff =
                    (next.contributions[opp] - next.contributions[next.current_player]).max(0);
                let is_wet = next.board.len() >= 3 && {
                    let mut suits = [0u8; 4];
                    for &c in next.board.as_slice() {
                        suits[c.suit as usize] += 1;
                    }
                    suits.iter().any(|&s| s >= 2)
                };
                let pot_with_call = pot + diff;
                let raise_amt = if next.round == 4 {
                    /* River: Full pot bet (100% pot) for polarized value and bluffing */
                    pot_with_call.max(RAISE_HALF_POT_FLOOR_RIVER)
                } else if next.round == 3 {
                    /* Turn: 75% pot bet for geometric pot growth */
                    (pot_with_call * 3 / 4).max(RAISE_HALF_POT_FLOOR_TURN)
                } else if is_wet {
                    /* Flop (wet): 75% pot bet */
                    (pot_with_call * 3 / 4).max(RAISE_HALF_POT_FLOOR_WET)
                } else {
                    /* Flop (dry) or Preflop: 50% pot bet */
                    (pot_with_call / 2).max(RAISE_HALF_POT_FLOOR_DRY)
                };
                next.contributions[next.current_player] =
                    (next.contributions[next.current_player] + diff + raise_amt).min(STACK_SIZE);
                next.raises_this_round += 1;
            }
            ALL_IN => {
                next.contributions[next.current_player] = STACK_SIZE;
                next.raises_this_round += 1;
            }
            _ => unreachable!(),
        }

        let hist = &next.history[r_idx];
        let n = hist.len();
        let round_over = n >= 2 && {
            let last = hist[n - 1];
            let prev = hist[n - 2];
            (last == CALL_CHECK && prev == CALL_CHECK) || (last == CALL_CHECK && prev >= RAISE_MIN)
        };

        if round_over {
            next.raises_this_round = 0;
            if next.contributions[0] >= STACK_SIZE && next.contributions[1] >= STACK_SIZE {
                while next.board.len() < 5 && !next.deck_remaining.is_empty() {
                    next.board.push(next.deck_remaining.deal_one());
                }
                next.round = 4;
                next.resolve_showdown();
                return next;
            }
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
