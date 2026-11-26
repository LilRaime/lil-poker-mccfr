/*! Texas Hold'em Benchmark and A/B Evaluation Suite */
pub mod blind_schedule;
pub mod elo;
pub mod gauntlet;
pub mod hu;
pub mod multiway;
pub mod telemetry;

use std::collections::HashMap;

use rand::RngExt;
use rand::rngs::SmallRng;

use crate::cfr::opponent_model::GuardId;
use crate::game::config::NUM_CARDS_DECK;
use crate::game::holdem::{ALL_52_CARDS, Card};

pub use blind_schedule::{
    BlindScheduleConfig, BlindScheduleLevel, DEFAULT_BLIND_SCHEDULE,
    run_holdem_blind_schedule_benchmark,
};
pub use elo::{EloEntry, EloTournamentConfig, run_holdem_elo_tournament};
pub use gauntlet::{GauntletConfig, run_holdem_ablation_benchmark, run_holdem_gauntlet_benchmark};
pub use hu::{HoldemBenchmarkConfig, run_holdem_benchmark, run_holdem_benchmark_preloaded};
pub use multiway::{MultiwayBenchmarkConfig, run_holdem_multiway_benchmark};

/** Benchmark and Evaluation Result Structure */
#[derive(Debug, Clone, Default)]
pub struct HoldemBenchmarkResult {
    pub duplicate_mode: bool,
    pub total_hands: u64,
    pub total_deals: u64,
    pub net_chips: f64,
    pub edge_per_hand: f64,
    pub se_per_hand: f64,
    pub win_rate_bb100: f64,
    pub se_bb100: f64,
    pub ci_95_bb100: (f64, f64),
    pub win_rate_mbb: f64,
    pub se_mbb: f64,
    pub win_pct: f64,
    pub loss_pct: f64,
    pub tie_pct: f64,
    pub sb_win_rate_bb100: f64,
    pub bb_win_rate_bb100: f64,
    pub guard_telemetry: HashMap<GuardId, (u64, f64)>,
}

pub(super) fn deal_cards_prefix(rng: &mut SmallRng, cards_needed: usize) -> [Card; NUM_CARDS_DECK] {
    let mut deck = ALL_52_CARDS;
    for i in 0..cards_needed {
        let j = rng.random_range(i..NUM_CARDS_DECK);
        deck.swap(i, j);
    }
    deck
}
