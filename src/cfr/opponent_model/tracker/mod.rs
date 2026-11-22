/*! Opponent Action Tracking and Statistical Metrics */
mod metrics;
mod profile;

use crate::cfr::abstraction::{preflop_bucket, preflop_strength_tier};
use crate::game::config::{
    ALL_IN, CALL_CHECK, FOLD, RAISE_MIN, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER, ROUND_TURN,
};
use crate::game::holdem::{Card, evaluate_7cards};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default)]
    pub flop_checks: u64,
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
    #[serde(default)]
    pub postflop_overbets: u64,
    #[serde(default)]
    pub river_overbets: u64,
    #[serde(default)]
    pub postflop_bets_total: u64,
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

    /* Per-hand deduplication & aggression flags */
    pub hand_vpip_recorded: bool,
    pub hand_pfr_recorded: bool,
    pub hand_saw_flop_recorded: bool,
    #[serde(default)]
    pub hand_pf_raises: u8,
    #[serde(default)]
    pub hand_post_raises: u8,
    #[serde(default)]
    pub last_observed_bb: i32,

    /** Optional guard ablation filter (reason codes disabled for benchmarking) */
    #[serde(skip)]
    pub disabled_guards: Vec<String>,
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
            flop_checks: 0,
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
            postflop_overbets: 0,
            river_overbets: 0,
            postflop_bets_total: 0,
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
            hand_vpip_recorded: false,
            hand_pfr_recorded: false,
            hand_saw_flop_recorded: false,
            hand_pf_raises: 0,
            hand_post_raises: 0,
            last_observed_bb: 0,
            disabled_guards: Vec::new(),
        }
    }

    pub fn set_disabled_guards(&mut self, guards: Vec<String>) {
        self.disabled_guards = guards
            .into_iter()
            .map(|g| g.trim().to_ascii_uppercase())
            .filter(|g| !g.is_empty())
            .collect();
    }

    pub fn is_guard_enabled(&self, reason: &str) -> bool {
        if self.disabled_guards.is_empty() {
            return true;
        }
        let upper = reason.to_ascii_uppercase();
        !self
            .disabled_guards
            .iter()
            .any(|g| g == "ALL" || g == &upper)
    }

    pub fn set_exploit_enabled(&mut self, enabled: bool) {
        self.exploit_enabled = enabled;
    }

    pub fn set_exploit_factor(&mut self, factor: f64) {
        self.exploit_factor = factor.clamp(0.0, 1.0);
    }

    pub fn record_action(&mut self, action: u8, is_preflop: bool) {
        let round = if is_preflop {
            ROUND_PREFLOP
        } else {
            ROUND_FLOP
        };
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
            ROUND_PREFLOP => self.record_preflop_street_action(action, act_cat),
            ROUND_FLOP..=ROUND_RIVER => {
                self.record_postflop_street_action(action, round, act_cat, facing_bet);
            }
            _ => {}
        }

        if action >= RAISE_MIN && pot > 0 && bet_amt > 0 {
            self.record_bet_sizing_tell(round, bet_amt, pot);
        }

        match action {
            FOLD => self.fold_count += 1,
            CALL_CHECK => self.call_count += 1,
            RAISE_MIN..=ALL_IN => self.raise_count += 1,
            _ => {}
        }
    }

    fn record_preflop_street_action(&mut self, action: u8, act_cat: usize) {
        self.preflop_actions[act_cat] += 1;
        if action == CALL_CHECK {
            if !self.hand_vpip_recorded {
                self.vpip_hands += 1;
                self.hand_vpip_recorded = true;
            }
        } else if action >= RAISE_MIN {
            self.hand_pf_raises = self.hand_pf_raises.saturating_add(1);
            if !self.hand_vpip_recorded {
                self.vpip_hands += 1;
                self.hand_vpip_recorded = true;
            }
            if !self.hand_pfr_recorded {
                self.pfr_hands += 1;
                self.hand_pfr_recorded = true;
            }
        }
    }

    fn record_postflop_street_action(
        &mut self,
        action: u8,
        round: u8,
        act_cat: usize,
        facing_bet: bool,
    ) {
        if action >= RAISE_MIN {
            self.hand_post_raises = self.hand_post_raises.saturating_add(1);
        }
        match round {
            ROUND_FLOP => {
                if !self.hand_saw_flop_recorded {
                    self.saw_flop += 1;
                    self.hand_saw_flop_recorded = true;
                }
                self.flop_actions[act_cat] += 1;
                if action == CALL_CHECK && !facing_bet {
                    self.flop_checks += 1;
                }
                if facing_bet {
                    self.flop_cbet_faced += 1;
                    if action == FOLD {
                        self.flop_fold_to_cbet += 1;
                    }
                }
            }
            ROUND_TURN => {
                self.turn_actions[act_cat] += 1;
                if facing_bet {
                    self.turn_cbet_faced += 1;
                    if action == FOLD {
                        self.turn_fold_to_cbet += 1;
                    }
                }
            }
            ROUND_RIVER => {
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
    }

    fn record_bet_sizing_tell(&mut self, round: u8, bet_amt: i32, pot: i32) {
        let pct = (bet_amt as i64 * 100 / pot.max(1) as i64) as i32;
        if pct <= 35 {
            self.sizing_small_bets += 1;
        } else if pct <= 75 {
            self.sizing_mid_bets += 1;
        } else {
            self.sizing_large_bets += 1;
        }
        if round >= ROUND_FLOP {
            self.postflop_bets_total += 1;
            if (bet_amt as i64) * 10 > (pot as i64) * 12 {
                self.postflop_overbets += 1;
                if round == ROUND_RIVER {
                    self.river_overbets += 1;
                }
            }
        }
    }

    pub fn end_hand(&mut self) {
        self.total_hands += 1;
        self.hand_vpip_recorded = false;
        self.hand_pfr_recorded = false;
        self.hand_saw_flop_recorded = false;
        self.hand_pf_raises = 0;
        self.hand_post_raises = 0;
    }

    pub fn record_showdown(&mut self) {
        if !self.hand_saw_flop_recorded {
            self.saw_flop += 1;
            self.hand_saw_flop_recorded = true;
        }
        self.went_to_showdown += 1;
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
            if is_pfr && !self.hand_pfr_recorded {
                /* BB 3-bet / raise */
                self.pfr_hands += 1;
                self.hand_pfr_recorded = true;
            }
        }
    }

    pub fn record_facing_3bet(&mut self, folded: bool) {
        self.faced_3bet_count += 1;
        if folded {
            self.fold_to_3bet_count += 1;
        }
    }

    /** Analyzes opponent's revealed cards at Showdown for deep range and tendency inference */
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
        if !self.hand_saw_flop_recorded {
            self.saw_flop += 1;
            self.hand_saw_flop_recorded = true;
        }
        self.went_to_showdown += 1;
        let score = evaluate_7cards(&opp_hole, board);
        let cat = score >> 32;
        let (pf_idx, _) = preflop_bucket(opp_hole[0], opp_hole[1]);

        if opp_raised_river && cat <= 1 {
            self.showdown_bluff_count += 1;
        }
        if preflop_strength_tier(pf_idx) == 1
            && !opp_raised_river
            && last_bet_amt == 0
            && self.hand_pf_raises < 2
            && self.hand_post_raises == 0
        {
            self.showdown_trap_count += 1;
        }
        if preflop_strength_tier(pf_idx) >= 7 {
            self.showdown_loose_count += 1;
        }
        if pot > 0 && last_bet_amt > 0 {
            let pct = (last_bet_amt as i64 * 100 / pot.max(1) as i64) as i32;
            if pct >= 75 && cat >= 2 {
                self.showdown_large_monster_count += 1;
            } else if pct <= 35 && cat <= 1 {
                self.showdown_small_bluff_count += 1;
            }
        }
    }
}
