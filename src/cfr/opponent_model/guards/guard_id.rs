/*! Typed Guard Identifier (`#[repr(u8)]`) and Zero-Allocation Bitmask (`GuardSet`) */

use std::fmt;
use std::ops::{BitOr, BitOrAssign};
use std::str::FromStr;

/** Unique typed identifier for every tactical policy and execution guard rule */
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuardId {
    PfDisciplineGuard = 0,
    PfButtonSteal = 1,
    PfSbAggressorTrap = 2,
    PassiveStationBoost = 3,
    PotCommitEarly = 4,
    PostNutValueFloor = 5,
    RiverNutBlockerBluff = 6,
    FlopWetSetValue = 7,
    FlopValueCheckRaise = 8,
    PostPolarizedValueSizing = 9,
    RiverThinValueBet = 10,
    PostAllinBluffCap = 11,
    PostAirGutshotGuard = 12,
    ShoveDefenseFold = 13,
    RangeEquityShoveFold = 14,
    PostSubgameReraiseCap = 15,
    PostOverbetAllinCap = 16,
    PostRiverBluffCap = 17,
    PostRiverOverbetCap = 18,
    PotCommitTerminal = 19,
    TurnDrawOddsFold = 20,
    FlopDrawOddsFold = 21,
    PostPotControlCap = 22,
    PostPairedFlushStraightCap = 23,
    ThreeFlushNoRaise = 24,
    MultiwayAdapt = 25,
    PostBarrelFold = 26,
    OverbetBluffCatch = 27,
    RiverCheckToTurnAggressor = 28,
    FlopOopMicroDonkSuppress = 29,
    PotCommitDecisionRescue = 30,
    LowSprBluffSuppress = 31,
    LowSprAirCallFold = 32,
    ShortStackNashPush = 33,
    ShortStackBbIsoPush = 34,
    ShortStackBbFreeCheck = 35,
    ShortStackFold = 36,
    ShortStackBb3BetShove = 37,
    ShortStackBbFlatCall = 38,
    ShortStackPricedInCall = 39,
}

const _: () = assert!(GuardId::COUNT <= 64);

impl GuardId {
    pub const COUNT: usize = 40;

    pub const ALL: [GuardId; Self::COUNT] = [
        Self::PfDisciplineGuard,
        Self::PfButtonSteal,
        Self::PfSbAggressorTrap,
        Self::PassiveStationBoost,
        Self::PotCommitEarly,
        Self::PostNutValueFloor,
        Self::RiverNutBlockerBluff,
        Self::FlopWetSetValue,
        Self::FlopValueCheckRaise,
        Self::PostPolarizedValueSizing,
        Self::RiverThinValueBet,
        Self::PostAllinBluffCap,
        Self::PostAirGutshotGuard,
        Self::ShoveDefenseFold,
        Self::RangeEquityShoveFold,
        Self::PostSubgameReraiseCap,
        Self::PostOverbetAllinCap,
        Self::PostRiverBluffCap,
        Self::PostRiverOverbetCap,
        Self::PotCommitTerminal,
        Self::TurnDrawOddsFold,
        Self::FlopDrawOddsFold,
        Self::PostPotControlCap,
        Self::PostPairedFlushStraightCap,
        Self::ThreeFlushNoRaise,
        Self::MultiwayAdapt,
        Self::PostBarrelFold,
        Self::OverbetBluffCatch,
        Self::RiverCheckToTurnAggressor,
        Self::FlopOopMicroDonkSuppress,
        Self::PotCommitDecisionRescue,
        Self::LowSprBluffSuppress,
        Self::LowSprAirCallFold,
        Self::ShortStackNashPush,
        Self::ShortStackBbIsoPush,
        Self::ShortStackBbFreeCheck,
        Self::ShortStackFold,
        Self::ShortStackBb3BetShove,
        Self::ShortStackBbFlatCall,
        Self::ShortStackPricedInCall,
    ];

    /** Canonical telemetry / CLI identifier string for this guard */
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PfDisciplineGuard => "PF_DISCIPLINE_GUARD",
            Self::PfButtonSteal => "PF_BUTTON_STEAL",
            Self::PfSbAggressorTrap => "PF_SB_AGGRESSOR_TRAP",
            Self::PassiveStationBoost => "PASSIVE_STATION_BOOST",
            Self::PotCommitEarly => "POT_COMMIT_EARLY",
            Self::PostNutValueFloor => "POST_NUT_VALUE_FLOOR",
            Self::RiverNutBlockerBluff => "RIVER_NUT_BLOCKER_BLUFF",
            Self::FlopWetSetValue => "FLOP_WET_SET_VALUE",
            Self::FlopValueCheckRaise => "FLOP_VALUE_CHECK_RAISE",
            Self::PostPolarizedValueSizing => "POST_POLARIZED_VALUE_SIZING",
            Self::RiverThinValueBet => "RIVER_THIN_VALUE_BET",
            Self::PostAllinBluffCap => "POST_ALLIN_BLUFF_CAP",
            Self::PostAirGutshotGuard => "POST_AIR_GUTSHOT_GUARD",
            Self::ShoveDefenseFold => "SHOVE_DEFENSE_FOLD",
            Self::RangeEquityShoveFold => "RANGE_EQUITY_SHOVE_FOLD",
            Self::PostSubgameReraiseCap => "POST_SUBGAME_RERAISE_CAP",
            Self::PostOverbetAllinCap => "POST_OVERBET_ALLIN_CAP",
            Self::PostRiverBluffCap => "POST_RIVER_BLUFF_CAP",
            Self::PostRiverOverbetCap => "POST_RIVER_OVERBET_CAP",
            Self::PotCommitTerminal => "POT_COMMIT_TERMINAL",
            Self::TurnDrawOddsFold => "TURN_DRAW_ODDS_FOLD",
            Self::FlopDrawOddsFold => "FLOP_DRAW_ODDS_FOLD",
            Self::PostPotControlCap => "POST_POT_CONTROL_CAP",
            Self::PostPairedFlushStraightCap => "POST_PAIRED_FLUSH_STRAIGHT_CAP",
            Self::ThreeFlushNoRaise => "THREE_FLUSH_NO_RAISE",
            Self::MultiwayAdapt => "MULTIWAY_ADAPT",
            Self::PostBarrelFold => "POST_BARREL_FOLD",
            Self::OverbetBluffCatch => "OVERBET_BLUFF_CATCH",
            Self::RiverCheckToTurnAggressor => "RIVER_CHECK_TO_TURN_AGGRESSOR",
            Self::FlopOopMicroDonkSuppress => "FLOP_OOP_MICRO_DONK_SUPPRESS",
            Self::PotCommitDecisionRescue => "POT_COMMIT_DECISION_RESCUE",
            Self::LowSprBluffSuppress => "LOW_SPR_BLUFF_SUPPRESS",
            Self::LowSprAirCallFold => "LOW_SPR_AIR_CALL_FOLD",
            Self::ShortStackNashPush => "SHORT_STACK_NASH_PUSH",
            Self::ShortStackBbIsoPush => "SHORT_STACK_BB_ISO_PUSH",
            Self::ShortStackBbFreeCheck => "SHORT_STACK_BB_FREE_CHECK",
            Self::ShortStackFold => "SHORT_STACK_FOLD",
            Self::ShortStackBb3BetShove => "SHORT_STACK_BB_3BET_SHOVE",
            Self::ShortStackBbFlatCall => "SHORT_STACK_BB_FLAT_CALL",
            Self::ShortStackPricedInCall => "SHORT_STACK_PRICED_IN_CALL",
        }
    }

    /** Convert raw bit index (`0..Self::COUNT`) to `GuardId` */
    #[inline]
    pub const fn from_index(idx: u8) -> Option<Self> {
        if (idx as usize) < Self::ALL.len() {
            Some(Self::ALL[idx as usize])
        } else {
            None
        }
    }
}

impl fmt::Display for GuardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for GuardId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        for id in Self::ALL {
            if id.as_str().eq_ignore_ascii_case(trimmed) {
                return Ok(id);
            }
        }
        Err(format!("Unknown guard id: '{trimmed}'"))
    }
}

/** Zero-allocation bitset of active or disabled `GuardId`s backed by a single `u64` register */
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct GuardSet(u64);

impl GuardSet {
    pub const EMPTY: Self = Self(0);
    pub const ALL: Self = Self((1u64 << GuardId::COUNT) - 1);

    #[inline(always)]
    pub const fn new() -> Self {
        Self::EMPTY
    }

    #[inline(always)]
    pub const fn from_id(id: GuardId) -> Self {
        Self(1u64 << (id as u8))
    }

    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline(always)]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    #[inline(always)]
    pub const fn insert(&mut self, id: GuardId) {
        self.0 |= 1u64 << (id as u8);
    }

    #[inline(always)]
    pub const fn push(&mut self, id: GuardId) {
        self.insert(id);
    }

    #[inline(always)]
    pub const fn contains(self, id: GuardId) -> bool {
        (self.0 & (1u64 << (id as u8))) != 0
    }

    #[inline(always)]
    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    /** Parse a comma-separated list of guard reason codes (`"ALL"` enables all bits) */
    pub fn from_disable_spec(raw: &str) -> Result<Self, String> {
        let mut set = Self::EMPTY;
        for tok in raw.split(',') {
            let trimmed = tok.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.eq_ignore_ascii_case("ALL") {
                return Ok(Self::ALL);
            }
            set.insert(GuardId::from_str(trimmed)?);
        }
        Ok(set)
    }

    #[inline(always)]
    pub const fn iter(self) -> GuardSetIter {
        GuardSetIter(self.0)
    }
}

impl BitOr for GuardSet {
    type Output = Self;

    #[inline(always)]
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for GuardSet {
    #[inline(always)]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl IntoIterator for GuardSet {
    type Item = GuardId;
    type IntoIter = GuardSetIter;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl fmt::Debug for GuardSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.iter().map(GuardId::as_str))
            .finish()
    }
}

/** Zero-allocation bit-scan iterator over set bits in `GuardSet` */
#[derive(Debug, Clone, Copy)]
pub struct GuardSetIter(u64);

impl Iterator for GuardSetIter {
    type Item = GuardId;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        while self.0 != 0 {
            let idx = self.0.trailing_zeros() as u8;
            self.0 &= self.0 - 1;
            if let Some(id) = GuardId::from_index(idx) {
                return Some(id);
            }
        }
        None
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let count = self.0.count_ones() as usize;
        (count, Some(count))
    }
}
