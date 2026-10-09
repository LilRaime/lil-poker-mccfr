/* Opponent Action Tracking and Statistical Metrics */
use super::classifier::OpponentStyle;
use crate::cfr::abstraction::preflop_bucket;
use crate::game::holdem::{evaluate_7cards, Card};

#[derive(Debug, Clone)]
pub struct OpponentTracker {
    pub total_hands: u64,
    pub vpip_hands: u64,
    pub pfr_hands: u64,
    pub fold_count: u64,
    pub call_count: u64,
    pub raise_count: u64,

    /* Street-specific action counters [fold, call, raise] */
    pub preflop_actions: [u64; 3],
    pub flop_actions: [u64; 3],
    pub turn_actions: [u64; 3],
    pub river_actions: [u64; 3],

    /* Advanced situational metrics */
    pub flop_cbet_faced: u64,
    pub flop_fold_to_cbet: u64,
    pub went_to_showdown: u64,
    pub saw_flop: u64,

    /* Showdown observation analytics */
    pub showdown_hands: u64,
    pub showdown_bluff_count: u64,
    pub showdown_trap_count: u64,
    pub showdown_loose_count: u64,
}

impl Default for OpponentTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl OpponentTracker {
    pub fn new() -> Self {
        OpponentTracker {
            total_hands: 0,
            vpip_hands: 0,
            pfr_hands: 0,
            fold_count: 0,
            call_count: 0,
            raise_count: 0,
            preflop_actions: [0; 3],
            flop_actions: [0; 3],
            turn_actions: [0; 3],
            river_actions: [0; 3],
            flop_cbet_faced: 0,
            flop_fold_to_cbet: 0,
            went_to_showdown: 0,
            saw_flop: 0,
            showdown_hands: 0,
            showdown_bluff_count: 0,
            showdown_trap_count: 0,
            showdown_loose_count: 0,
        }
    }

    pub fn record_action(&mut self, action: u8, is_preflop: bool) {
        let round = if is_preflop { 1 } else { 2 };
        self.record_action_street(action, round, false);
    }

    pub fn record_action_street(&mut self, action: u8, round: u8, facing_cbet: bool) {
        let act_cat = match action {
            0 => 0,     /* Fold */
            1 => 1,     /* Call / Check */
            2..=5 => 2, /* Raise / Bet */
            _ => 1,
        };

        match round {
            1 => {
                self.preflop_actions[act_cat] += 1;
                if action == 1 {
                    self.vpip_hands += 1;
                } else if action >= 2 {
                    self.vpip_hands += 1;
                    self.pfr_hands += 1;
                }
            }
            2 => {
                self.saw_flop += 1;
                self.flop_actions[act_cat] += 1;
                if facing_cbet {
                    self.flop_cbet_faced += 1;
                    if action == 0 {
                        self.flop_fold_to_cbet += 1;
                    }
                }
            }
            3 => self.turn_actions[act_cat] += 1,
            4 => self.river_actions[act_cat] += 1,
            _ => {}
        }

        match action {
            0 => self.fold_count += 1,
            1 => self.call_count += 1,
            2..=5 => self.raise_count += 1,
            _ => {}
        }
    }

    pub fn end_hand(&mut self) {
        self.total_hands += 1;
    }

    pub fn record_showdown(&mut self) {
        self.went_to_showdown += 1;
    }

    pub fn total_actions(&self) -> u64 {
        self.fold_count + self.call_count + self.raise_count
    }

    pub fn fold_ratio(&self) -> f64 {
        let tot = self.total_actions();
        if tot == 0 {
            0.33
        } else {
            self.fold_count as f64 / tot as f64
        }
    }

    pub fn call_ratio(&self) -> f64 {
        let tot = self.total_actions();
        if tot == 0 {
            0.33
        } else {
            self.call_count as f64 / tot as f64
        }
    }

    pub fn raise_ratio(&self) -> f64 {
        let tot = self.total_actions();
        if tot == 0 {
            0.33
        } else {
            self.raise_count as f64 / tot as f64
        }
    }

    pub fn vpip_ratio(&self) -> f64 {
        if self.total_hands == 0 {
            0.50
        } else {
            self.vpip_hands as f64 / self.total_hands as f64
        }
    }

    pub fn pfr_ratio(&self) -> f64 {
        if self.total_hands == 0 {
            0.25
        } else {
            self.pfr_hands as f64 / self.total_hands as f64
        }
    }

    pub fn flop_fold_to_cbet_ratio(&self) -> f64 {
        if self.flop_cbet_faced == 0 {
            0.40
        } else {
            self.flop_fold_to_cbet as f64 / self.flop_cbet_faced as f64
        }
    }

    pub fn wtsd_ratio(&self) -> f64 {
        if self.saw_flop == 0 {
            0.30
        } else {
            self.went_to_showdown as f64 / self.saw_flop as f64
        }
    }

    /* Analyzes opponent's revealed cards at Showdown for deep range and tendency inference */
    pub fn record_showdown_hand(
        &mut self,
        opp_hole: [Card; 2],
        board: &[Card],
        opp_raised_river: bool,
    ) {
        self.showdown_hands += 1;
        self.went_to_showdown += 1;
        let score = evaluate_7cards(&opp_hole, board);
        let cat = score >> 32;
        let (pf_idx, _) = preflop_bucket(opp_hole[0], opp_hole[1]);

        /* 1. Was opponent caught bluffing with weak hand / air on river? */
        if opp_raised_river && cat <= 1 {
            self.showdown_bluff_count += 1;
        }
        /* 2. Did opponent slowplay / trap with monster preflop hand? */
        if pf_idx <= 5 {
            self.showdown_trap_count += 1;
        }
        /* 3. Did opponent enter pot with trash cards? */
        if pf_idx > 90 {
            self.showdown_loose_count += 1;
        }
    }

    pub fn showdown_bluff_rate(&self) -> f64 {
        if self.showdown_hands == 0 {
            0.20
        } else {
            self.showdown_bluff_count as f64 / self.showdown_hands as f64
        }
    }

    /* Classifies opponent playstyle into a discrete archetype with confidence */
    pub fn classify_style(&self) -> (OpponentStyle, f64) {
        if self.total_hands < 3 {
            return (OpponentStyle::Unknown, 0.0);
        }
        let vpip = self.vpip_ratio();
        let pfr = self.pfr_ratio();
        let fold = self.fold_ratio();
        let raise = self.raise_ratio();

        let conf = (self.total_hands as f64 / (self.total_hands as f64 + 7.0)).min(0.95);

        if vpip > 0.60 && pfr < 0.20 && fold < 0.28 {
            (OpponentStyle::CallingStation, conf)
        } else if pfr > 0.38 && raise > 0.35 {
            (OpponentStyle::Maniac, conf)
        } else if vpip < 0.22 && fold > 0.45 {
            (OpponentStyle::Rock, conf)
        } else if (0.18..=0.35).contains(&vpip) && pfr >= 0.14 {
            (OpponentStyle::Tag, conf)
        } else if (0.35..=0.65).contains(&vpip) {
            (OpponentStyle::Lag, conf)
        } else {
            (OpponentStyle::CallingStation, conf * 0.7)
        }
    }
}
