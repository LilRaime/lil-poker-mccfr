/*! Opponent Archetype, Statistical Style Classification, and Unified Tactical Profile */
use super::tracker::OpponentTracker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpponentStyle {
    Unknown,
    CallingStation,
    Maniac,
    Rock,
    Tag,
    Lag,
}

impl std::fmt::Display for OpponentStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpponentStyle::Unknown => write!(f, "Unknown"),
            OpponentStyle::CallingStation => write!(f, "Calling Station (Loose-Passive)"),
            OpponentStyle::Maniac => write!(f, "Maniac (Hyper-Aggressive)"),
            OpponentStyle::Rock => write!(f, "Rock / Nit (Tight-Passive)"),
            OpponentStyle::Tag => write!(f, "TAG (Tight-Aggressive)"),
            OpponentStyle::Lag => write!(f, "LAG (Loose-Aggressive)"),
        }
    }
}

/** Single source of truth for behavioral tactical flags consumed by guards,
 * short-stack Nash logic, and post-selection adjustments. */
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TacticalProfile {
    pub is_passive_station: bool,
    pub is_sticky_caller: bool,
    pub is_overbet_bluffer: bool,
    pub is_known_maniac: bool,
    pub is_extreme_shove_bot: bool,
    pub is_hyper_aggressor: bool,
    pub is_tight_passive_bb: bool,
    pub is_ultra_sticky_bb: bool,
    pub is_tight_passive_postflop: bool,
    pub is_low_pfr_aggressor: bool,
    pub is_frequent_sb_limper: bool,
}

impl OpponentTracker {
    /** Computes all unified behavioral opponent flags in a single pass. */
    pub fn tactical_profile(&self) -> TacticalProfile {
        let sm_raise = self.smoothed_raise_ratio();
        let sm_pfr = self.smoothed_pfr();
        let sm_vpip = self.smoothed_vpip();
        let sm_fold = self.smoothed_fold_ratio();

        let is_passive_station = self.is_passive_station();
        let is_sticky_caller = is_passive_station
            || (self.flop_cbet_faced >= 4 && self.smoothed_flop_fold_to_cbet() < 0.28)
            || (self.turn_cbet_faced + self.river_bet_faced >= 4
                && self.turn_fold_to_cbet_ratio() < 0.30)
            || (self.total_actions() >= 15 && sm_fold < 0.14 && sm_raise <= 0.22);
        let is_overbet_bluffer = self.is_overbet_bluffer();
        let is_known_maniac = self.total_actions() >= 10
            && (self.total_hands < 6 || sm_pfr >= 0.26)
            && ((sm_raise > 0.42 && sm_vpip >= 0.52)
                || (sm_vpip > 0.65 && sm_raise > 0.38)
                || (sm_pfr > 0.48 && sm_raise > 0.32)
                || (sm_vpip > 0.80 && sm_raise > 0.28));
        let is_extreme_shove_bot = self.total_hands >= 8
            && (self.total_hands < 6 || sm_pfr >= 0.26)
            && sm_raise > 0.42
            && sm_vpip > 0.68;
        let is_hyper_aggressor = self.total_hands >= 12
            && (self.bb_total_hands < 5 || self.bb_vpip_ratio() >= 0.45)
            && ((self.raise_ratio() > 0.48 && self.vpip_ratio() >= 0.50)
                || (self.pfr_ratio() > 0.52 && self.raise_ratio() > 0.36));
        let is_tight_passive_bb = (self.total_hands >= 8
            && ((sm_vpip < 0.30 && sm_pfr < 0.18)
                || (self.vpip_ratio() < 0.28 && self.pfr_ratio() < 0.18)))
            || (self.bb_total_hands >= 6 && self.bb_vpip_ratio() <= 0.42);
        let is_ultra_sticky_bb = self.bb_total_hands >= 8 && self.bb_vpip_ratio() >= 0.80;
        let is_tight_passive_postflop = self.total_hands >= 8
            && ((sm_vpip < 0.30 && sm_pfr < 0.18)
                || (self.vpip_ratio() < 0.26 && self.pfr_ratio() < 0.16));
        let is_low_pfr_aggressor = self.total_hands >= 10 && sm_pfr < 0.22 && !is_overbet_bluffer;
        let is_frequent_sb_limper = self.sb_total_hands >= 5 && self.sb_limp_ratio() > 0.30;

        TacticalProfile {
            is_passive_station,
            is_sticky_caller,
            is_overbet_bluffer,
            is_known_maniac,
            is_extreme_shove_bot,
            is_hyper_aggressor,
            is_tight_passive_bb,
            is_ultra_sticky_bb,
            is_tight_passive_postflop,
            is_low_pfr_aggressor,
            is_frequent_sb_limper,
        }
    }
}
