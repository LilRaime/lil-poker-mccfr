use std::collections::HashMap;

use rand::rngs::SmallRng;
use rand::SeedableRng;

use super::super::engine::{simulate_holdem_hand_traced, SimHandConfig};
use super::super::strategy::{load_holdem_strategy, HoldemStrategy};
use super::{deal_cards_prefix, HoldemBenchmarkResult};
use crate::cfr::fallback::StrategyLookupIndex;
use crate::cfr::opponent_model::OpponentTracker;
use crate::cfr::subgame::SubgameSolver;
use crate::game::config::{
    BIG_BLIND, BIG_BLIND_F64, NUM_COMMUNITY_CARDS, NUM_HOLE_CARDS, NUM_PLAYERS,
};
use crate::game::holdem::TexasHoldemGame;
use crate::sim::stats::SimStats;

const CARDS_PER_DEAL: usize = NUM_PLAYERS * NUM_HOLE_CARDS + NUM_COMMUNITY_CARDS;

#[derive(Debug, Clone, Copy)]
pub struct BlindScheduleConfig<'a> {
    pub strategy_path: &'a str,
    pub total_deals: u64,
    pub hands_per_level: u64,
    pub enable_subgame_search: bool,
    pub opp_archetype: &'a str,
    pub opp_strategy_path: Option<&'a str>,
    pub exploit_factor: f64,
    pub seed: Option<u64>,
    pub disabled_guards: &'a [String],
    pub quiet: bool,
}

impl Default for BlindScheduleConfig<'_> {
    fn default() -> Self {
        Self {
            strategy_path: "",
            total_deals: 50,
            hands_per_level: 20,
            enable_subgame_search: false,
            opp_archetype: "tag",
            opp_strategy_path: None,
            exploit_factor: 0.70,
            seed: None,
            disabled_guards: &[],
            quiet: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BlindScheduleLevel {
    pub name: &'static str,
    pub stack_bb: u32,
    pub effective_bb: i32,
}

pub const DEFAULT_BLIND_SCHEDULE: [BlindScheduleLevel; 5] = [
    BlindScheduleLevel {
        name: "L1 (BB=100, 100BB)",
        stack_bb: 100,
        effective_bb: 100,
    },
    BlindScheduleLevel {
        name: "L2 (BB=200,  50BB)",
        stack_bb: 50,
        effective_bb: 200,
    },
    BlindScheduleLevel {
        name: "L3 (BB=400,  25BB)",
        stack_bb: 25,
        effective_bb: 400,
    },
    BlindScheduleLevel {
        name: "L4 (BB=600,  16BB)",
        stack_bb: 16,
        effective_bb: 600,
    },
    BlindScheduleLevel {
        name: "L5 (BB=1000, 10BB)",
        stack_bb: 10,
        effective_bb: 1000,
    },
];

fn load_blind_schedule_strategies(
    cfg: &BlindScheduleConfig<'_>,
) -> (Option<HoldemStrategy>, Option<HoldemStrategy>) {
    let strategy = load_holdem_strategy(cfg.strategy_path);
    let opp_strategy = if let Some(path) = cfg.opp_strategy_path {
        load_holdem_strategy(path)
    } else if cfg.opp_archetype.eq_ignore_ascii_case("self") {
        strategy.clone()
    } else {
        None
    };
    (strategy, opp_strategy)
}

/** Rising-BB Tournament / Blind-Schedule Benchmark:
 * Plays sequential duplicate matches where Big Blind steps up every `hands_per_level` hands
 * (default 20 hands/level = 10 duplicate deals/level across 5 levels = 100-hand matches).
 * Tracks opponent stats across each match and notifies `observe_blind_level` at each BB jump. */
pub fn run_holdem_blind_schedule_benchmark(cfg: &BlindScheduleConfig<'_>) -> HoldemBenchmarkResult {
    let (strategy, opp_strategy) = load_blind_schedule_strategies(cfg);
    let strategy_index = strategy.as_ref().map(StrategyLookupIndex::build_from_map);

    let deals_per_level = (cfg.hands_per_level.max(2) / 2).max(1);
    let num_levels = DEFAULT_BLIND_SCHEDULE.len();
    let deals_per_match = deals_per_level * num_levels as u64;
    let total_deals = cfg.total_deals.max(deals_per_match);

    let mut rng = match cfg.seed {
        Some(s) => SmallRng::seed_from_u64(s),
        None => SmallRng::from_entropy(),
    };
    let subgame_solver = SubgameSolver::default();
    let base_hand_cfg = SimHandConfig {
        my_player: 0,
        strategy: strategy.as_ref(),
        strategy_index: strategy_index.as_ref(),
        opp_archetype: cfg.opp_archetype,
        opp_strategy: opp_strategy.as_ref(),
        subgame_solver: Some(&subgame_solver),
        enable_subgame_search: cfg.enable_subgame_search,
        ..Default::default()
    };

    let mut opp_tracker = OpponentTracker::new();
    opp_tracker.set_exploit_factor(cfg.exploit_factor);
    opp_tracker.set_disabled_guards(cfg.disabled_guards.to_vec());

    let mut total_stats = SimStats::default();
    let mut duplicate_stats = SimStats::default();
    let mut pos_stats = [SimStats::default(), SimStats::default()];
    let mut level_stats: Vec<SimStats> = vec![SimStats::default(); num_levels];
    let mut match_bb_net = 0.0f64;
    let mut match_wins = 0u64;
    let mut total_matches = 0u64;
    let mut last_lvl_idx: Option<usize> = None;

    for ep in 0..total_deals {
        let match_offset = ep % deals_per_match;
        if match_offset == 0 {
            opp_tracker = OpponentTracker::new();
            opp_tracker.set_exploit_factor(cfg.exploit_factor);
            opp_tracker.set_disabled_guards(cfg.disabled_guards.to_vec());
            match_bb_net = 0.0;
            last_lvl_idx = None;
        }
        let lvl_idx = (match_offset / deals_per_level) as usize;
        let level = DEFAULT_BLIND_SCHEDULE[lvl_idx];
        if last_lvl_idx != Some(lvl_idx) {
            opp_tracker.observe_blind_level(level.effective_bb);
            last_lvl_idx = Some(lvl_idx);
        }

        let deck = deal_cards_prefix(&mut rng, CARDS_PER_DEAL);
        let hole_a = [deck[0], deck[1]];
        let hole_b = [deck[2], deck[3]];
        let runout = &deck[4..CARDS_PER_DEAL];
        let stack_chips = (level.stack_bb as i32) * BIG_BLIND;

        let game1 = TexasHoldemGame::new_dealt_with_stack(hole_a, hole_b, runout, stack_chips);
        let (ret1, _) =
            simulate_holdem_hand_traced(game1, &base_hand_cfg, &mut opp_tracker, &mut rng);
        let game2 = TexasHoldemGame::new_dealt_with_stack(hole_a, hole_b, runout, stack_chips);
        let (ret2, _) = simulate_holdem_hand_traced(
            game2,
            &SimHandConfig {
                my_player: 1,
                ..base_hand_cfg
            },
            &mut opp_tracker,
            &mut rng,
        );

        /* Record returns scaled relative to current blind level BB while weighting duplicate edge */
        let ret1_bb_scaled = ret1 * (level.effective_bb as f64 / BIG_BLIND_F64);
        let ret2_bb_scaled = ret2 * (level.effective_bb as f64 / BIG_BLIND_F64);
        pos_stats[0].record(ret1);
        pos_stats[1].record(ret2);
        total_stats.record(ret1);
        total_stats.record(ret2);
        level_stats[lvl_idx].record(ret1);
        level_stats[lvl_idx].record(ret2);
        duplicate_stats.record(ret1 + ret2);

        match_bb_net += (ret1_bb_scaled + ret2_bb_scaled) / BIG_BLIND_F64;
        if match_offset + 1 == deals_per_match {
            total_matches += 1;
            if match_bb_net > 0.0 {
                match_wins += 1;
            }
        }
    }

    summarize_blind_schedule_result(
        cfg,
        &BlindScheduleSummary {
            deals_per_level,
            total_deals,
            total_matches,
            match_wins,
            level_stats: &level_stats,
            pos_stats: &pos_stats,
            total_stats: &total_stats,
            duplicate_stats: &duplicate_stats,
        },
    )
}

struct BlindScheduleSummary<'a> {
    deals_per_level: u64,
    total_deals: u64,
    total_matches: u64,
    match_wins: u64,
    level_stats: &'a [SimStats],
    pos_stats: &'a [SimStats; 2],
    total_stats: &'a SimStats,
    duplicate_stats: &'a SimStats,
}

fn summarize_blind_schedule_result(
    cfg: &BlindScheduleConfig<'_>,
    s: &BlindScheduleSummary<'_>,
) -> HoldemBenchmarkResult {
    let avg_chips = s.duplicate_stats.avg_chips() / 2.0;
    let se_chips = s.duplicate_stats.se_chips() / 2.0;
    let win_rate_bb100 = (avg_chips / BIG_BLIND_F64) * 100.0;
    let se_bb100 = (se_chips / BIG_BLIND_F64) * 100.0;
    let ci_low = win_rate_bb100 - 1.96 * se_bb100;
    let ci_high = win_rate_bb100 + 1.96 * se_bb100;
    let sb_bb100 = (s.pos_stats[0].avg_chips() / BIG_BLIND_F64) * 100.0;
    let bb_bb100 = (s.pos_stats[1].avg_chips() / BIG_BLIND_F64) * 100.0;

    if !cfg.quiet {
        println!("\n============================================================");
        println!(
            "   ♠️  Rising-BB Blind Schedule Benchmark ({} hands/level)  ♥️",
            s.deals_per_level * 2
        );
        println!("============================================================");
        println!(
            "Opponent : {} | Deals : {} ({} hands, {} matches)",
            cfg.opp_archetype.to_uppercase(),
            s.total_deals,
            s.total_deals * 2,
            s.total_matches
        );
        for (idx, level) in DEFAULT_BLIND_SCHEDULE.iter().enumerate() {
            let st = &s.level_stats[idx];
            let l_bb100 = (st.avg_chips() / BIG_BLIND_F64) * 100.0;
            let l_se = (st.se_chips() / BIG_BLIND_F64) * 100.0;
            println!(
                "  {:<22} | {:>+8.2} bb/100 (±{:.2}) | {} hands",
                level.name, l_bb100, l_se, st.hands
            );
        }
        println!("------------------------------------------------------------");
        println!(
            "Overall Average   : {win_rate_bb100:>+8.2} bb/100 (95% CI [{ci_low:>+7.2}, {ci_high:>+7.2}])"
        );
        if s.total_matches > 0 {
            println!(
                "Match Win Rate    : {}/{} ({:.1}%)",
                s.match_wins,
                s.total_matches,
                100.0 * s.match_wins as f64 / s.total_matches as f64
            );
        }
        println!("============================================================\n");
    }

    HoldemBenchmarkResult {
        duplicate_mode: true,
        total_hands: s.total_stats.hands,
        total_deals: s.total_deals,
        net_chips: s.total_stats.total_chips,
        edge_per_hand: avg_chips,
        se_per_hand: se_chips,
        win_rate_bb100,
        se_bb100,
        ci_95_bb100: (ci_low, ci_high),
        win_rate_mbb: win_rate_bb100 * 10.0,
        se_mbb: se_bb100 * 10.0,
        win_pct: s.total_stats.win_pct(),
        loss_pct: s.total_stats.loss_pct(),
        tie_pct: s.total_stats.tie_pct(),
        sb_win_rate_bb100: sb_bb100,
        bb_win_rate_bb100: bb_bb100,
        guard_telemetry: HashMap::new(),
    }
}
