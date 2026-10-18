/* Opponent Action Tracking and Statistical Metrics */
use super::classifier::OpponentStyle;
use crate::cfr::abstraction::preflop_bucket;
use crate::game::config::*;
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
    pub turn_cbet_faced: u64,
    pub turn_fold_to_cbet: u64,
    pub river_bet_faced: u64,
    pub river_fold_to_bet: u64,
    pub went_to_showdown: u64,
    pub saw_flop: u64,

    /* Sizing tells metrics */
    pub sizing_small_bets: u64,
    pub sizing_mid_bets: u64,
    pub sizing_large_bets: u64,
    pub showdown_small_bluff_count: u64,
    pub showdown_large_monster_count: u64,

    /* Showdown observation analytics */
    pub showdown_hands: u64,
    pub showdown_bluff_count: u64,
    pub showdown_trap_count: u64,
    pub showdown_loose_count: u64,

    /* Positional & 3-bet situational metrics */
    pub sb_vpip_hands: u64,
    pub sb_pfr_hands: u64,
    pub sb_total_hands: u64,
    pub bb_vpip_hands: u64,
    pub bb_total_hands: u64,
    pub faced_3bet_count: u64,
    pub fold_to_3bet_count: u64,

    /* Exploitation guardrails & configuration */
    pub exploit_enabled: bool,
    pub exploit_factor: f64,
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
            turn_cbet_faced: 0,
            turn_fold_to_cbet: 0,
            river_bet_faced: 0,
            river_fold_to_bet: 0,
            went_to_showdown: 0,
            saw_flop: 0,
            sizing_small_bets: 0,
            sizing_mid_bets: 0,
            sizing_large_bets: 0,
            showdown_small_bluff_count: 0,
            showdown_large_monster_count: 0,
            showdown_hands: 0,
            showdown_bluff_count: 0,
            showdown_trap_count: 0,
            showdown_loose_count: 0,
            sb_vpip_hands: 0,
            sb_pfr_hands: 0,
            sb_total_hands: 0,
            bb_vpip_hands: 0,
            bb_total_hands: 0,
            faced_3bet_count: 0,
            fold_to_3bet_count: 0,
            exploit_enabled: true,
            exploit_factor: 0.70,
        }
    }

    pub fn set_exploit_enabled(&mut self, enabled: bool) {
        self.exploit_enabled = enabled;
    }

    pub fn set_exploit_factor(&mut self, factor: f64) {
        self.exploit_factor = factor.clamp(0.0, 1.0);
    }

    pub fn record_action(&mut self, action: u8, is_preflop: bool) {
        let round = if is_preflop { 1 } else { 2 };
        self.record_action_street(action, round, false);
    }

    pub fn record_action_street(&mut self, action: u8, round: u8, facing_cbet: bool) {
        self.record_action_street_detailed(action, round, facing_cbet, 0, 0);
    }

    pub fn record_action_street_detailed(
        &mut self,
        action: u8,
        round: u8,
        facing_bet: bool,
        bet_amt: i32,
        pot: i32,
    ) {
        let act_cat = match action {
            FOLD => 0,               /* Fold */
            CALL_CHECK => 1,         /* Call / Check */
            RAISE_MIN..=ALL_IN => 2, /* Raise / Bet */
            _ => 1,
        };

        match round {
            1 => {
                self.preflop_actions[act_cat] += 1;
                if action == CALL_CHECK {
                    self.vpip_hands += 1;
                } else if action >= RAISE_MIN {
                    self.vpip_hands += 1;
                    self.pfr_hands += 1;
                }
            }
            2 => {
                self.saw_flop += 1;
                self.flop_actions[act_cat] += 1;
                if facing_bet {
                    self.flop_cbet_faced += 1;
                    if action == FOLD {
                        self.flop_fold_to_cbet += 1;
                    }
                }
            }
            3 => {
                self.turn_actions[act_cat] += 1;
                if facing_bet {
                    self.turn_cbet_faced += 1;
                    if action == FOLD {
                        self.turn_fold_to_cbet += 1;
                    }
                }
            }
            4 => {
                self.river_actions[act_cat] += 1;
                if facing_bet {
                    self.river_bet_faced += 1;
                    if action == FOLD {
                        self.river_fold_to_bet += 1;
                    }
                }
            }
            _ => {}
        }

        if action >= RAISE_MIN && pot > 0 && bet_amt > 0 {
            let pct = (bet_amt as i64 * 100 / pot.max(1) as i64) as i32;
            if pct <= 35 {
                self.sizing_small_bets += 1;
            } else if pct <= 75 {
                self.sizing_mid_bets += 1;
            } else {
                self.sizing_large_bets += 1;
            }
        }

        match action {
            FOLD => self.fold_count += 1,
            CALL_CHECK => self.call_count += 1,
            RAISE_MIN..=ALL_IN => self.raise_count += 1,
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

    pub fn turn_fold_to_cbet_ratio(&self) -> f64 {
        if self.turn_cbet_faced == 0 {
            0.40
        } else {
            self.turn_fold_to_cbet as f64 / self.turn_cbet_faced as f64
        }
    }

    pub fn river_fold_to_bet_ratio(&self) -> f64 {
        if self.river_bet_faced == 0 {
            0.35
        } else {
            self.river_fold_to_bet as f64 / self.river_bet_faced as f64
        }
    }

    pub fn wtsd_ratio(&self) -> f64 {
        if self.saw_flop == 0 {
            0.30
        } else {
            self.went_to_showdown as f64 / self.saw_flop as f64
        }
    }

    pub fn sb_vpip_ratio(&self) -> f64 {
        if self.sb_total_hands == 0 {
            0.70
        } else {
            self.sb_vpip_hands as f64 / self.sb_total_hands as f64
        }
    }

    pub fn sb_pfr_ratio(&self) -> f64 {
        if self.sb_total_hands == 0 {
            0.50
        } else {
            self.sb_pfr_hands as f64 / self.sb_total_hands as f64
        }
    }

    pub fn bb_vpip_ratio(&self) -> f64 {
        if self.bb_total_hands == 0 {
            0.55
        } else {
            self.bb_vpip_hands as f64 / self.bb_total_hands as f64
        }
    }

    pub fn fold_to_3bet_ratio(&self) -> f64 {
        if self.faced_3bet_count == 0 {
            0.50
        } else {
            self.fold_to_3bet_count as f64 / self.faced_3bet_count as f64
        }
    }

    pub fn sb_limp_ratio(&self) -> f64 {
        if self.sb_total_hands == 0 {
            0.0
        } else {
            (self.sb_vpip_hands.saturating_sub(self.sb_pfr_hands)) as f64
                / self.sb_total_hands as f64
        }
    }

    pub fn flop_check_ratio(&self) -> f64 {
        let total = self.flop_actions[1] + self.flop_actions[2];
        if total == 0 {
            0.50
        } else {
            self.flop_actions[1] as f64 / total as f64
        }
    }

    pub fn record_preflop_positional(&mut self, is_sb: bool, is_vpip: bool, is_pfr: bool) {
        if is_sb {
            self.sb_total_hands += 1;
            if is_vpip {
                self.sb_vpip_hands += 1;
            }
            if is_pfr {
                self.sb_pfr_hands += 1;
            }
        } else {
            self.bb_total_hands += 1;
            if is_vpip {
                self.bb_vpip_hands += 1;
            }
            if is_pfr {
                /* BB 3-bet / raise */
                self.pfr_hands += 1;
            }
        }
    }

    pub fn record_facing_3bet(&mut self, folded: bool) {
        self.faced_3bet_count += 1;
        if folded {
            self.fold_to_3bet_count += 1;
        }
    }

    /* Analyzes opponent's revealed cards at Showdown for deep range and tendency inference */
    pub fn record_showdown_hand(
        &mut self,
        opp_hole: [Card; 2],
        board: &[Card],
        opp_raised_river: bool,
    ) {
        self.record_showdown_hand_detailed(opp_hole, board, opp_raised_river, 0, 0);
    }

    pub fn record_showdown_hand_detailed(
        &mut self,
        opp_hole: [Card; 2],
        board: &[Card],
        opp_raised_river: bool,
        last_bet_amt: i32,
        pot: i32,
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
        /* 4. Sizing tells inference */
        if pot > 0 && last_bet_amt > 0 {
            let pct = (last_bet_amt as i64 * 100 / pot.max(1) as i64) as i32;
            if pct >= 75 && cat >= 2 {
                self.showdown_large_monster_count += 1;
            } else if pct <= 35 && cat <= 1 {
                self.showdown_small_bluff_count += 1;
            }
        }
    }

    pub fn showdown_bluff_rate(&self) -> f64 {
        if self.showdown_hands == 0 {
            0.20
        } else {
            self.showdown_bluff_count as f64 / self.showdown_hands as f64
        }
    }

    /* Wilson score interval for statistical significance of frequency bounds.
     * Returns (lower_bound, upper_bound) at confidence level z (e.g. z=1.96 for 95%). */
    pub fn wilson_confidence(successes: u64, total: u64, z: f64) -> (f64, f64) {
        if total == 0 {
            return (0.0, 1.0);
        }
        let n = total as f64;
        let p_hat = successes as f64 / n;
        let z2 = z * z;
        let denom = 1.0 + z2 / n;
        let center = (p_hat + z2 / (2.0 * n)) / denom;
        let margin = (z / denom)
            * ((p_hat * (1.0 - p_hat) / n) + (z2 / (4.0 * n * n)))
                .max(0.0)
                .sqrt();
        ((center - margin).max(0.0), (center + margin).min(1.0))
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
