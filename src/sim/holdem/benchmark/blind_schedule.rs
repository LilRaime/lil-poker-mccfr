use std::collections::HashMap;

use rand::rngs::SmallRng;
use rand::SeedableRng;

use super::super::engine::simulate_holdem_hand_traced;
use super::super::strategy::load_holdem_strategy;
use super::{deal_cards_prefix, HoldemBenchmarkResult};
use crate::cfr::fallback::StrategyLookupIndex;
use crate::cfr::opponent_model::OpponentTracker;
use crate::cfr::subgame::SubgameSolver;
use crate::game::config::*;
use crate::game::holdem::TexasHoldemGame;
use crate::sim::stats::SimStats;

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

/* Rising-BB Tournament / Blind-Schedule Benchmark:
 * Plays sequential duplicate matches where Big Blind steps up every `hands_per_level` hands
 * (default 20 hands/level = 10 duplicate deals/level across 5 levels = 100-hand matches).
 * Tracks opponent stats across each match and notifies `observe_blind_level` at each BB jump. */
#[allow(clippy::too_many_arguments)]
pub fn run_holdem_blind_schedule_benchmark(
    strategy_path: &str,
    total_deals: u64,
    hands_per_level: u64,
    enable_subgame_search: bool,
    opp_archetype: &str,
    opp_strategy_path: Option<&str>,
    exploit_factor: f64,
    seed: Option<u64>,
    disabled_guards: &[String],
    quiet: bool,
) -> HoldemBenchmarkResult {
    let strategy = load_holdem_strategy(strategy_path);
    let strategy_index = strategy.as_ref().map(StrategyLookupIndex::build_from_map);
    let opp_strategy = if let Some(path) = opp_strategy_path {
        load_holdem_strategy(path)
    } else if opp_archetype.eq_ignore_ascii_case("self") {
        strategy.clone()
    } else {
        None
    };

    let deals_per_level = (hands_per_level.max(2) / 2).max(1);
    let num_levels = DEFAULT_BLIND_SCHEDULE.len();
    let deals_per_match = deals_per_level * num_levels as u64;
    let total_deals = total_deals.max(deals_per_match);

    let mut rng = match seed {
        Some(s) => SmallRng::seed_from_u64(s),
        None => SmallRng::from_entropy(),
    };
    let subgame_solver = SubgameSolver::default();

    let mut opp_tracker = OpponentTracker::new();
    opp_tracker.set_exploit_factor(exploit_factor);
    opp_tracker.set_disabled_guards(disabled_guards.to_vec());

    let mut total_stats = SimStats::default();
    let mut duplicate_stats = SimStats::default();
    let mut pos_stats = [SimStats::default(), SimStats::default()];
    let mut level_stats: Vec<SimStats> = vec![SimStats::default(); num_levels];
    let mut match_bb_net = 0.0f64;
    let mut match_wins = 0u64;
    let mut total_matches = 0u64;
    let mut last_lvl_idx: Option<usize> = None;

    const CARDS_NEEDED: usize = NUM_PLAYERS * NUM_HOLE_CARDS + NUM_COMMUNITY_CARDS;
    for ep in 0..total_deals {
        let match_offset = ep % deals_per_match;
        if match_offset == 0 {
            opp_tracker = OpponentTracker::new();
            opp_tracker.set_exploit_factor(exploit_factor);
            opp_tracker.set_disabled_guards(disabled_guards.to_vec());
            match_bb_net = 0.0;
            last_lvl_idx = None;
        }
        let lvl_idx = (match_offset / deals_per_level) as usize;
        let level = DEFAULT_BLIND_SCHEDULE[lvl_idx];
        if last_lvl_idx != Some(lvl_idx) {
            opp_tracker.observe_blind_level(level.effective_bb);
            last_lvl_idx = Some(lvl_idx);
        }

        let deck = deal_cards_prefix(&mut rng, CARDS_NEEDED);
        let hole_a = [deck[0], deck[1]];
        let hole_b = [deck[2], deck[3]];
        let runout = &deck[4..CARDS_NEEDED];
        let stack_chips = (level.stack_bb as i32) * BIG_BLIND;

        let game1 = TexasHoldemGame::new_dealt_with_stack(hole_a, hole_b, runout, stack_chips);
        let (ret1, _) = simulate_holdem_hand_traced(
            game1,
            0,
            &strategy,
            strategy_index.as_ref(),
            opp_archetype,
            &opp_strategy,
            &subgame_solver,
            enable_subgame_search,
            &mut opp_tracker,
            &mut rng,
            0,
            false,
            "",
        );
        let game2 = TexasHoldemGame::new_dealt_with_stack(hole_a, hole_b, runout, stack_chips);
        let (ret2, _) = simulate_holdem_hand_traced(
            game2,
            1,
            &strategy,
            strategy_index.as_ref(),
            opp_archetype,
            &opp_strategy,
            &subgame_solver,
            enable_subgame_search,
            &mut opp_tracker,
            &mut rng,
            0,
            false,
            "",
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

    let avg_chips = duplicate_stats.avg_chips() / 2.0;
    let se_chips = duplicate_stats.se_chips() / 2.0;
    let win_rate_bb100 = (avg_chips / BIG_BLIND_F64) * 100.0;
    let se_bb100 = (se_chips / BIG_BLIND_F64) * 100.0;
    let ci_low = win_rate_bb100 - 1.96 * se_bb100;
    let ci_high = win_rate_bb100 + 1.96 * se_bb100;
    let sb_bb100 = (pos_stats[0].avg_chips() / BIG_BLIND_F64) * 100.0;
    let bb_bb100 = (pos_stats[1].avg_chips() / BIG_BLIND_F64) * 100.0;

    if !quiet {
        println!("\n============================================================");
        println!(
            "   ♠️  Rising-BB Blind Schedule Benchmark ({} hands/level)  ♥️",
            deals_per_level * 2
        );
        println!("============================================================");
        println!(
            "Opponent : {} | Deals : {} ({} hands, {} matches)",
            opp_archetype.to_uppercase(),
            total_deals,
            total_deals * 2,
            total_matches
        );
        for (idx, level) in DEFAULT_BLIND_SCHEDULE.iter().enumerate() {
            let st = &level_stats[idx];
            let l_bb100 = (st.avg_chips() / BIG_BLIND_F64) * 100.0;
            let l_se = (st.se_chips() / BIG_BLIND_F64) * 100.0;
            println!(
                "  {:<22} | {:>+8.2} bb/100 (±{:.2}) | {} hands",
                level.name, l_bb100, l_se, st.hands
            );
        }
        println!("------------------------------------------------------------");
        println!(
            "Overall Average   : {:>+8.2} bb/100 (95% CI [{:>+7.2}, {:>+7.2}])",
            win_rate_bb100, ci_low, ci_high
        );
        if total_matches > 0 {
            println!(
                "Match Win Rate    : {}/{} ({:.1}%)",
                match_wins,
                total_matches,
                100.0 * match_wins as f64 / total_matches as f64
            );
        }
        println!("============================================================\n");
    }

    HoldemBenchmarkResult {
        duplicate_mode: true,
        total_hands: total_stats.hands,
        total_deals,
        net_chips: total_stats.total_chips,
        edge_per_hand: avg_chips,
        se_per_hand: se_chips,
        win_rate_bb100,
        se_bb100,
        ci_95_bb100: (ci_low, ci_high),
        win_rate_mbb: win_rate_bb100 * 10.0,
        se_mbb: se_bb100 * 10.0,
        win_pct: total_stats.win_pct(),
        loss_pct: total_stats.loss_pct(),
        tie_pct: total_stats.tie_pct(),
        sb_win_rate_bb100: sb_bb100,
        bb_win_rate_bb100: bb_bb100,
        guard_telemetry: HashMap::new(),
    }
}
