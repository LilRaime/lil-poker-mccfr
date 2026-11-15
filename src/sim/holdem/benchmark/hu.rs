use std::collections::HashMap;

use rand::rngs::SmallRng;
use rand::SeedableRng;

use super::super::engine::simulate_holdem_hand_traced;
use super::super::strategy::{load_holdem_strategy, HoldemStrategy};
use super::telemetry::{
    print_guard_telemetry, print_opponent_tracker_summary, print_positional_breakdown,
    record_guard_reasons,
};
use super::{deal_cards_prefix, HoldemBenchmarkResult};
use crate::cfr::fallback::StrategyLookupIndex;
use crate::cfr::opponent_model::OpponentTracker;
use crate::cfr::subgame::SubgameSolver;
use crate::game::config::*;
use crate::game::holdem::TexasHoldemGame;
use crate::sim::stats::SimStats;

/* Backward-compatible simulation entrypoint used by play CLI */
#[allow(clippy::too_many_arguments)]
pub fn run_holdem_episodes_log_mode(
    strategy_path: &str,
    total_episodes: u64,
    delay_ms: u64,
    enable_subgame_search: bool,
    player_arg: &str,
    opp_archetype: &str,
    opp_strategy_path: Option<&str>,
    duplicate_mode: bool,
) {
    let _ = run_holdem_benchmark(
        strategy_path,
        total_episodes,
        delay_ms,
        enable_subgame_search,
        player_arg,
        opp_archetype,
        opp_strategy_path,
        duplicate_mode,
        0.70,
        false,
    );
}

/* Full 52-Card Texas Hold'em Benchmark and A/B Evaluation Suite */
#[allow(clippy::too_many_arguments)]
pub fn run_holdem_benchmark(
    strategy_path: &str,
    total_episodes: u64,
    delay_ms: u64,
    enable_subgame_search: bool,
    player_arg: &str,
    opp_archetype: &str,
    opp_strategy_path: Option<&str>,
    duplicate_mode: bool,
    exploit_factor: f64,
    quiet: bool,
) -> HoldemBenchmarkResult {
    run_holdem_benchmark_seeded(
        strategy_path,
        total_episodes,
        delay_ms,
        enable_subgame_search,
        player_arg,
        opp_archetype,
        opp_strategy_path,
        duplicate_mode,
        exploit_factor,
        quiet,
        None,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_holdem_benchmark_seeded(
    strategy_path: &str,
    total_episodes: u64,
    delay_ms: u64,
    enable_subgame_search: bool,
    player_arg: &str,
    opp_archetype: &str,
    opp_strategy_path: Option<&str>,
    duplicate_mode: bool,
    exploit_factor: f64,
    quiet: bool,
    seed: Option<u64>,
    disabled_guards: &[String],
) -> HoldemBenchmarkResult {
    run_holdem_benchmark_with_stack(
        strategy_path,
        total_episodes,
        delay_ms,
        enable_subgame_search,
        player_arg,
        opp_archetype,
        opp_strategy_path,
        duplicate_mode,
        exploit_factor,
        quiet,
        seed,
        disabled_guards,
        STACK_SIZE,
    )
}

fn load_strategies_and_log(
    strategy_path: &str,
    opp_archetype: &str,
    opp_strategy_path: Option<&str>,
    duplicate_mode: bool,
    enable_subgame_search: bool,
    stack_size: i32,
    quiet: bool,
) -> (Option<HoldemStrategy>, Option<HoldemStrategy>) {
    let strategy = load_holdem_strategy(strategy_path);
    if !quiet {
        if strategy.is_some() {
            println!("[holdem] Loaded bot strategy from '{}'", strategy_path);
        } else {
            println!(
                "[holdem] WARNING: strategy file '{}' not found — bot plays using fallback.",
                strategy_path
            );
        }
    }

    let opp_strategy = if let Some(path) = opp_strategy_path {
        let loaded = load_holdem_strategy(path);
        if !quiet {
            if loaded.is_some() {
                println!("[holdem] Loaded opponent strategy from '{}'", path);
            } else {
                eprintln!(
                    "[holdem] WARNING: Strategy file '{}' not found or invalid JSON — opponent falls back to '{}' archetype.",
                    path,
                    opp_archetype.to_uppercase()
                );
            }
        }
        loaded
    } else if opp_archetype.eq_ignore_ascii_case("self") {
        strategy.clone()
    } else {
        None
    };

    if !quiet {
        if let (Some(_), Some(opp_path)) = (&opp_strategy, opp_strategy_path) {
            println!("\nA/B BENCHMARK: '{}' vs '{}'", strategy_path, opp_path);
        } else if opp_archetype.eq_ignore_ascii_case("self") {
            println!("\nSELF-PLAY BENCHMARK: '{}' vs Self", strategy_path);
        } else {
            println!(
                "[holdem] Opponent archetype: '{}'",
                opp_archetype.to_uppercase()
            );
        }
        if duplicate_mode {
            println!("[duplicate] DUPLICATE POKER ACTIVE: Each deal played in reverse seats to eliminate card luck!");
        }
        if stack_size != STACK_SIZE {
            println!(
                "[stack] Effective Stack Depth: {} chips ({} BB)",
                stack_size,
                stack_size / BIG_BLIND
            );
        }
        if enable_subgame_search {
            println!("[subgame] Real-Time Search (Subgame Solving) enabled on Turn & River.");
        }
    }
    (strategy, opp_strategy)
}

fn summarize_duplicate_result(
    duplicate_stats: &SimStats,
    total_stats: &SimStats,
    opp_strategy: &Option<HoldemStrategy>,
    opp_strategy_path: Option<&str>,
    opp_archetype: &str,
    quiet: bool,
) -> HoldemBenchmarkResult {
    let bb_val = BIG_BLIND_F64;
    let edge_per_hand = duplicate_stats.avg_chips() / 2.0;
    let se_per_hand = duplicate_stats.se_chips() / 2.0;
    let win_rate_bb100 = (edge_per_hand / bb_val) * 100.0;
    let se_bb100 = (se_per_hand / bb_val) * 100.0;
    let ci_low = win_rate_bb100 - 1.96 * se_bb100;
    let ci_high = win_rate_bb100 + 1.96 * se_bb100;
    let win_rate_mbb = (edge_per_hand / bb_val) * 1000.0;
    let se_mbb = (se_per_hand / bb_val) * 1000.0;

    if !quiet {
        println!("\n============================================================");
        println!("        ♠️  Texas Hold'em: Duplicate Poker Benchmark  ♥️      ");
        println!("============================================================");
        if let (Some(_), Some(opp_path)) = (opp_strategy, opp_strategy_path) {
            println!("A/B Matchup           : Bot vs '{}'", opp_path);
        } else if opp_archetype.eq_ignore_ascii_case("self") {
            println!("A/B Matchup           : Self-Play (Bot vs Self)");
        } else {
            println!("Opponent Archetype    : {}", opp_archetype.to_uppercase());
        }
        println!("Total Duplicate Deals : {}", duplicate_stats.hands);
        println!("Total Matches Played  : {}", total_stats.hands);
        println!(
            "Bot Net Duplicate Edge: {:+.1} chips",
            duplicate_stats.total_chips
        );
        println!(
            "Duplicate Deal Wins   : Wins: {} ({:.1}%) | Losses: {} ({:.1}%) | Ties: {} ({:.1}%)",
            duplicate_stats.wins,
            duplicate_stats.win_pct(),
            duplicate_stats.losses,
            duplicate_stats.loss_pct(),
            duplicate_stats.ties,
            duplicate_stats.tie_pct(),
        );
        println!(
            "Skill Edge per Hand   : {:+.3} chips (±{:.3})",
            edge_per_hand, se_per_hand
        );
        println!(
            "Duplicate Win Rate    : {:+.2} bb/100 (±{:.2})",
            win_rate_bb100, se_bb100,
        );
        println!(
            "95% Confidence Interval: [{:+.2}, {:+.2}] bb/100",
            ci_low, ci_high
        );
        println!(
            "Duplicate Win Rate    : {:+.1} mbb/hand (±{:.1})",
            win_rate_mbb, se_mbb,
        );
        if ci_low > 0.0 {
            println!("Statistical Verdict   : ✅ STATISTICALLY SIGNIFICANT +EV (p < 0.05)");
        } else if ci_high < 0.0 {
            println!("Statistical Verdict   : ❌ STATISTICALLY SIGNIFICANT -EV (p < 0.05)");
        } else {
            println!("Statistical Verdict   : ⚪ INCONCLUSIVE (overlaps zero — increase hands for higher statistical power)");
        }
    }

    HoldemBenchmarkResult {
        duplicate_mode: true,
        total_deals: duplicate_stats.hands,
        total_hands: total_stats.hands,
        net_chips: duplicate_stats.total_chips,
        edge_per_hand,
        se_per_hand,
        win_rate_bb100,
        se_bb100,
        ci_95_bb100: (ci_low, ci_high),
        win_rate_mbb,
        se_mbb,
        win_pct: duplicate_stats.win_pct(),
        loss_pct: duplicate_stats.loss_pct(),
        tie_pct: duplicate_stats.tie_pct(),
        ..Default::default()
    }
}

fn summarize_standard_result(
    total_stats: &SimStats,
    opp_archetype: &str,
    quiet: bool,
) -> HoldemBenchmarkResult {
    let bb_val = BIG_BLIND_F64;
    let avg = total_stats.avg_chips();
    let se = total_stats.se_chips();
    let win_rate_bb100 = (avg / bb_val) * 100.0;
    let se_bb100 = (se / bb_val) * 100.0;
    let ci_low = win_rate_bb100 - 1.96 * se_bb100;
    let ci_high = win_rate_bb100 + 1.96 * se_bb100;
    let win_rate_mbb = (avg / bb_val) * 1000.0;
    let se_mbb = (se / bb_val) * 1000.0;

    if !quiet {
        println!("\n============================================================");
        println!("        ♠️  Texas Hold'em: Bot Simulation Summary  ♥️        ");
        println!("============================================================");
        println!("Opponent Archetype  : {}", opp_archetype.to_uppercase());
        println!("Total Hands Played  : {}", total_stats.hands);
        println!(
            "Total Net Profit    : {:+.1} chips",
            total_stats.total_chips
        );
        println!(
            "Results Breakdown   : Wins: {} ({:.1}%) | Losses: {} ({:.1}%) | Ties: {} ({:.1}%)",
            total_stats.wins,
            total_stats.win_pct(),
            total_stats.losses,
            total_stats.loss_pct(),
            total_stats.ties,
            total_stats.tie_pct(),
        );
        println!("Average Profit      : {:+.3} chips/hand (±{:.3})", avg, se);
        println!(
            "Win Rate (BB/100)   : {:+.2} bb/100 (±{:.2})",
            win_rate_bb100, se_bb100,
        );
        println!(
            "95% Confidence Interval: [{:+.2}, {:+.2}] bb/100",
            ci_low, ci_high
        );
        println!(
            "Win Rate (mbb/hand) : {:+.1} mbb/hand (±{:.1})",
            win_rate_mbb, se_mbb,
        );
    }

    HoldemBenchmarkResult {
        duplicate_mode: false,
        total_hands: total_stats.hands,
        net_chips: total_stats.total_chips,
        edge_per_hand: avg,
        se_per_hand: se,
        win_rate_bb100,
        se_bb100,
        ci_95_bb100: (ci_low, ci_high),
        win_rate_mbb,
        se_mbb,
        win_pct: total_stats.win_pct(),
        loss_pct: total_stats.loss_pct(),
        tie_pct: total_stats.tie_pct(),
        ..Default::default()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_holdem_benchmark_with_stack(
    strategy_path: &str,
    total_episodes: u64,
    delay_ms: u64,
    enable_subgame_search: bool,
    player_arg: &str,
    opp_archetype: &str,
    opp_strategy_path: Option<&str>,
    duplicate_mode: bool,
    exploit_factor: f64,
    quiet: bool,
    seed: Option<u64>,
    disabled_guards: &[String],
    stack_size: i32,
) -> HoldemBenchmarkResult {
    let stack_size = stack_size.max(BIG_BLIND * 2);
    let (strategy, opp_strategy) = load_strategies_and_log(
        strategy_path,
        opp_archetype,
        opp_strategy_path,
        duplicate_mode,
        enable_subgame_search,
        stack_size,
        quiet,
    );
    let strategy_index = strategy.as_ref().map(StrategyLookupIndex::build_from_map);

    let mut rng = match seed {
        Some(s) => SmallRng::seed_from_u64(s),
        None => SmallRng::from_entropy(),
    };
    let mut opp_tracker = OpponentTracker::new();
    opp_tracker.set_exploit_factor(exploit_factor);
    opp_tracker.set_disabled_guards(disabled_guards.to_vec());
    let subgame_solver = SubgameSolver::default();

    let mut total_stats = SimStats::default();
    let mut pos_stats = [SimStats::default(), SimStats::default()];
    let mut duplicate_stats = SimStats::default();
    let mut guard_telemetry: HashMap<String, (u64, f64)> = HashMap::new();

    if duplicate_mode {
        let show_logs = !quiet && (delay_ms > 0 || total_episodes <= 10);
        const CARDS_NEEDED: usize = NUM_PLAYERS * NUM_HOLE_CARDS + NUM_COMMUNITY_CARDS;
        for ep in 1..=total_episodes {
            let deck = deal_cards_prefix(&mut rng, CARDS_NEEDED);
            let hole_a = [deck[0], deck[1]];
            let hole_b = [deck[2], deck[3]];
            let runout = &deck[4..CARDS_NEEDED];

            if show_logs {
                println!("\n=== Deal {}/{} (Duplicate) ===", ep, total_episodes);
            }

            let game1 = TexasHoldemGame::new_dealt_with_stack(hole_a, hole_b, runout, stack_size);
            let (ret1, reasons1) = simulate_holdem_hand_traced(
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
                delay_ms,
                show_logs,
                &format!("Deal {}/{} - Seat A (Bot as SB)", ep, total_episodes),
            );
            pos_stats[0].record(ret1);
            total_stats.record(ret1);
            record_guard_reasons(&mut guard_telemetry, reasons1, ret1);

            let game2 = TexasHoldemGame::new_dealt_with_stack(hole_a, hole_b, runout, stack_size);
            let (ret2, reasons2) = simulate_holdem_hand_traced(
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
                delay_ms,
                show_logs,
                &format!("Deal {}/{} - Seat B (Bot as BB)", ep, total_episodes),
            );
            pos_stats[1].record(ret2);
            total_stats.record(ret2);
            record_guard_reasons(&mut guard_telemetry, reasons2, ret2);

            let net_edge = ret1 + ret2;
            duplicate_stats.record(net_edge);

            if !quiet && (show_logs || ep % 200 == 0 || ep == total_episodes) {
                println!(
                    "Deal {:>4}/{} | SB: {:>+6.1} | BB: {:>+6.1} | Duplicate Edge: {:>+6.1} chips",
                    ep, total_episodes, ret1, ret2, net_edge
                );
            }
        }
    } else {
        for ep in 1..=total_episodes {
            let my_player = match player_arg {
                "0" | "sb" => 0usize,
                "1" | "bb" => 1usize,
                _ => ((ep - 1) % 2) as usize,
            };
            let game = TexasHoldemGame::new_random(&mut rng).with_stack(stack_size);
            let (ret, reasons) = simulate_holdem_hand_traced(
                game,
                my_player,
                &strategy,
                strategy_index.as_ref(),
                opp_archetype,
                &opp_strategy,
                &subgame_solver,
                enable_subgame_search,
                &mut opp_tracker,
                &mut rng,
                delay_ms,
                !quiet,
                &format!("Episode {}", ep),
            );
            pos_stats[my_player].record(ret);
            total_stats.record(ret);
            record_guard_reasons(&mut guard_telemetry, reasons, ret);
        }
    }

    if !quiet {
        print_opponent_tracker_summary(&opp_tracker);
    }

    let mut res = if duplicate_mode {
        summarize_duplicate_result(
            &duplicate_stats,
            &total_stats,
            &opp_strategy,
            opp_strategy_path,
            opp_archetype,
            quiet,
        )
    } else {
        summarize_standard_result(&total_stats, opp_archetype, quiet)
    };

    if pos_stats[0].hands > 0 {
        res.sb_win_rate_bb100 = (pos_stats[0].avg_chips() / BIG_BLIND_F64) * 100.0;
    }
    if pos_stats[1].hands > 0 {
        res.bb_win_rate_bb100 = (pos_stats[1].avg_chips() / BIG_BLIND_F64) * 100.0;
    }

    if !quiet {
        print_positional_breakdown(&pos_stats);
        if !guard_telemetry.is_empty() {
            print_guard_telemetry(&guard_telemetry, total_stats.hands.max(1));
        }
        println!("============================================================\n");
    }
    res.guard_telemetry = guard_telemetry;
    res
}
