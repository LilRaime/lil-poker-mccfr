/* Texas Hold'em Benchmark and A/B Evaluation Suite */
pub mod blind_schedule;
pub mod gauntlet;
pub mod hu;
pub mod multiway;
pub mod telemetry;

use std::collections::HashMap;

use rand::rngs::SmallRng;
use rand::Rng;

use crate::game::config::NUM_CARDS_DECK;
use crate::game::holdem::{Card, ALL_52_CARDS};

pub use blind_schedule::{
    run_holdem_blind_schedule_benchmark, BlindScheduleLevel, DEFAULT_BLIND_SCHEDULE,
};
pub use gauntlet::{
    run_holdem_ablation_benchmark, run_holdem_gauntlet_benchmark,
    run_holdem_gauntlet_benchmark_seeded, run_holdem_gauntlet_benchmark_with_stack,
};
pub use hu::{
    run_holdem_benchmark, run_holdem_benchmark_seeded, run_holdem_benchmark_with_stack,
    run_holdem_episodes_log_mode,
};
pub use multiway::{run_holdem_multiway_benchmark, run_holdem_multiway_benchmark_with_stack};

/* Benchmark and Evaluation Result Structure */
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
    pub guard_telemetry: HashMap<String, (u64, f64)>,
}

pub(super) fn deal_cards_prefix(rng: &mut SmallRng, cards_needed: usize) -> [Card; NUM_CARDS_DECK] {
    let mut deck = ALL_52_CARDS;
    for i in 0..cards_needed {
        let j = rng.gen_range(i..NUM_CARDS_DECK);
        deck.swap(i, j);
    }
    deck
}
