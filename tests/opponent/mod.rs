mod guards_postflop;
mod guards_preflop;
mod guards_subgame;
mod metrics_and_style;
mod sizing_and_station;

use lil_poker_mccfr::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};
use lil_poker_mccfr::cfr::opponent_model::{
    DecisionTrace, GuardSet, OpponentTracker, PolicyRequest,
};
use lil_poker_mccfr::game::config::HOLDEM_NUM_ACTIONS;
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, Card, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use lil_poker_mccfr::live::parse_card_str;
use rand::rngs::SmallRng;

pub(crate) const LEGAL_FULL: [u8; 6] = [
    FOLD,
    CALL_CHECK,
    RAISE_MIN,
    RAISE_THIRD_POT,
    RAISE_HALF_POT,
    ALL_IN,
];
pub(crate) const LEGAL_OPEN: [u8; 5] = [
    CALL_CHECK,
    RAISE_MIN,
    RAISE_THIRD_POT,
    RAISE_HALF_POT,
    ALL_IN,
];
pub(crate) const LEGAL_SHOVE: [u8; 3] = [FOLD, CALL_CHECK, ALL_IN];
pub(crate) const LEGAL_BB: [u8; 4] = [FOLD, CALL_CHECK, RAISE_MIN, ALL_IN];

pub(crate) const PROBS_CALL_HEAVY: [f64; HOLDEM_NUM_ACTIONS] = [0.05, 0.90, 0.0, 0.0, 0.0, 0.05];
pub(crate) const PROBS_FOLD_HEAVY: [f64; HOLDEM_NUM_ACTIONS] = [0.85, 0.15, 0.0, 0.0, 0.0, 0.0];
pub(crate) const PROBS_OPEN_JAM: [f64; HOLDEM_NUM_ACTIONS] = [0.0, 0.10, 0.05, 0.15, 0.0, 0.70];
pub(crate) const PROBS_RERAISE_HEAVY: [f64; HOLDEM_NUM_ACTIONS] =
    [0.0, 0.15, 0.15, 0.25, 0.25, 0.20];

pub(crate) fn card(s: &str) -> Card {
    parse_card_str(s).unwrap_or_else(|| panic!("invalid card token: {s}"))
}

pub(crate) fn hole(s: &str) -> [Card; 2] {
    let cards: Vec<Card> = s.split_whitespace().map(card).collect();
    [cards[0], cards[1]]
}

pub(crate) fn board(s: &str) -> Vec<Card> {
    if s.trim().is_empty() {
        Vec::new()
    } else {
        s.split_whitespace().map(card).collect()
    }
}

#[derive(Clone)]
pub(crate) struct Spot {
    pub hole_cards: Option<[Card; 2]>,
    pub board_cards: Vec<Card>,
    pub bucket: usize,
    pub round: u8,
    pub to_call: i32,
    pub pot: i32,
    pub active_opponents: usize,
    pub table_bb: i32,
    pub my_chips: i32,
    pub opp_chips: Option<i32>,
    pub from_subgame: bool,
}

impl Spot {
    pub(crate) fn preflop(hole_str: &str) -> Self {
        let h = hole(hole_str);
        let bucket = preflop_bucket(h[0], h[1]).0;
        Self {
            hole_cards: Some(h),
            board_cards: Vec::new(),
            bucket,
            round: 1,
            to_call: 0,
            pot: 0,
            active_opponents: 1,
            table_bb: 100,
            my_chips: 0,
            opp_chips: None,
            from_subgame: false,
        }
    }

    pub(crate) fn post(hole_str: &str, board_str: &str) -> Self {
        let h = hole(hole_str);
        let b = board(board_str);
        let round = match b.len() {
            3 => 2,
            4 => 3,
            _ => 4,
        };
        let bucket = postflop_equity_bucket(&h, &b);
        Self {
            hole_cards: Some(h),
            board_cards: b,
            bucket,
            round,
            to_call: 0,
            pot: 0,
            active_opponents: 1,
            table_bb: 100,
            my_chips: 0,
            opp_chips: None,
            from_subgame: false,
        }
    }

    pub(crate) fn bucket_only(round: u8, bucket: usize) -> Self {
        Self {
            hole_cards: None,
            board_cards: Vec::new(),
            bucket,
            round,
            to_call: 0,
            pot: 0,
            active_opponents: 1,
            table_bb: 100,
            my_chips: 0,
            opp_chips: None,
            from_subgame: false,
        }
    }

    pub(crate) fn pot(mut self, to_call: i32, pot: i32) -> Self {
        self.to_call = to_call;
        self.pot = pot;
        self
    }

    pub(crate) fn stacks(mut self, bb: i32, my_chips: i32) -> Self {
        self.table_bb = bb;
        self.my_chips = my_chips;
        self
    }

    pub(crate) fn opp(mut self, opp_chips: Option<i32>) -> Self {
        self.opp_chips = opp_chips;
        self
    }

    pub(crate) fn active(mut self, n: usize) -> Self {
        self.active_opponents = n;
        self
    }

    pub(crate) fn subgame(mut self) -> Self {
        self.from_subgame = true;
        self
    }

    pub(crate) fn once(
        &self,
        tracker: &OpponentTracker,
        probs: &[f64],
        legal: &[u8],
        rng: &mut SmallRng,
    ) -> (u8, DecisionTrace) {
        let board_opt = (!self.board_cards.is_empty()).then_some(self.board_cards.as_slice());
        tracker.select_action_traced(
            &PolicyRequest {
                raw_probs: probs,
                legal_actions: legal,
                bucket: self.bucket,
                round: self.round,
                to_call: self.to_call,
                pot: self.pot,
                hole: self.hole_cards.as_ref(),
                board: board_opt,
                active_opponents: self.active_opponents,
                table_bb: self.table_bb,
                my_chips: self.my_chips,
                opp_chips: self.opp_chips,
                from_subgame: self.from_subgame,
                ..Default::default()
            },
            rng,
        )
    }

    pub(crate) fn sample(
        &self,
        tracker: &OpponentTracker,
        probs: &[f64],
        legal: &[u8],
        trials: usize,
        rng: &mut SmallRng,
    ) -> ([usize; HOLDEM_NUM_ACTIONS], GuardSet) {
        let mut counts = [0usize; HOLDEM_NUM_ACTIONS];
        let mut reasons = GuardSet::default();
        for _ in 0..trials {
            let (act, tr) = self.once(tracker, probs, legal, rng);
            if let Some(slot) = counts.get_mut(act as usize) {
                *slot += 1;
            }
            reasons |= tr.guard_reasons;
        }
        (counts, reasons)
    }
}

pub(crate) fn make_shove_maniac() -> OpponentTracker {
    let mut maniac = OpponentTracker::new();
    for _ in 0..12 {
        maniac.record_action_street_detailed(ALL_IN, 1, false, 4000, 300);
        maniac.record_preflop_positional(true, true, true);
        maniac.end_hand();
    }
    maniac
}

pub(crate) fn make_aggro_maniac() -> OpponentTracker {
    let mut maniac = OpponentTracker::new();
    for _ in 0..12 {
        maniac.record_action_street_detailed(RAISE_HALF_POT, 1, false, 300, 450);
        maniac.record_action_street_detailed(RAISE_HALF_POT, 2, false, 600, 1200);
        maniac.end_hand();
    }
    maniac
}

pub(crate) fn make_ultra_nit() -> OpponentTracker {
    let mut ultra_nit = OpponentTracker::new();
    for i in 0..15 {
        if i == 0 {
            ultra_nit.record_action_street_detailed(RAISE_MIN, 1, false, 300, 150);
        } else if i < 3 {
            ultra_nit.record_action_street_detailed(CALL_CHECK, 1, false, 150, 150);
        } else {
            ultra_nit.record_action_street_detailed(FOLD, 1, true, 0, 300);
        }
        ultra_nit.end_hand();
    }
    ultra_nit
}

pub(crate) fn make_overbet_bluffer() -> OpponentTracker {
    let mut villain = OpponentTracker::new();
    for _ in 0..5 {
        villain.record_action_street_detailed(RAISE_HALF_POT, 2, false, 1500, 1000);
        villain.end_hand();
    }
    villain
}

pub(crate) fn make_passive_station() -> OpponentTracker {
    let mut station = OpponentTracker::new();
    for _ in 0..15 {
        station.record_action_street(CALL_CHECK, 1, false);
        station.record_action_street(CALL_CHECK, 2, false);
        station.end_hand();
    }
    station
}

pub(crate) fn make_calling_station() -> OpponentTracker {
    let mut tracker = OpponentTracker::new();
    for _ in 0..15 {
        tracker.record_action(CALL_CHECK, false);
    }
    tracker.record_action(FOLD, false);
    tracker.record_action(RAISE_MIN, false);
    tracker
}
