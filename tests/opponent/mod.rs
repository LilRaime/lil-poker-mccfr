mod guards_postflop;
mod guards_preflop;
mod guards_subgame;
mod metrics_and_style;
mod sizing_and_station;

use lil_poker_mccfr::cfr::opponent_model::OpponentTracker;
use lil_poker_mccfr::game::holdem::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};

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
