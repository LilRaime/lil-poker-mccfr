use std::collections::HashMap;

use super::hu::run_holdem_benchmark_with_stack;
use super::telemetry::print_guard_telemetry;
use super::HoldemBenchmarkResult;
use crate::game::config::{BIG_BLIND, STACK_SIZE};

/* Multi-Archetype Gauntlet Benchmark Suite */
pub fn run_holdem_gauntlet_benchmark(
    strategy_path: &str,
    deals_per_archetype: u64,
    enable_subgame_search: bool,
    exploit_factor: f64,
) -> Vec<(String, HoldemBenchmarkResult)> {
    run_holdem_gauntlet_benchmark_seeded(
        strategy_path,
        deals_per_archetype,
        enable_subgame_search,
        exploit_factor,
        Some(2026),
        &[],
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_holdem_gauntlet_benchmark_seeded(
    strategy_path: &str,
    deals_per_archetype: u64,
    enable_subgame_search: bool,
    exploit_factor: f64,
    base_seed: Option<u64>,
    disabled_guards: &[String],
    quiet: bool,
) -> Vec<(String, HoldemBenchmarkResult)> {
    run_holdem_gauntlet_benchmark_with_stack(
        strategy_path,
        deals_per_archetype,
        enable_subgame_search,
        exploit_factor,
        base_seed,
        disabled_guards,
        quiet,
        STACK_SIZE,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_holdem_gauntlet_benchmark_with_stack(
    strategy_path: &str,
    deals_per_archetype: u64,
    enable_subgame_search: bool,
    exploit_factor: f64,
    base_seed: Option<u64>,
    disabled_guards: &[String],
    quiet: bool,
    stack_size: i32,
) -> Vec<(String, HoldemBenchmarkResult)> {
    let stack_size = stack_size.max(BIG_BLIND * 2);
    let archetypes = [
        "tag",
        "calling_station",
        "maniac",
        "shove_bot",
        "trapper",
        "rock",
        "self",
    ];
    let mut results = Vec::with_capacity(archetypes.len());
    let mut combined_telemetry: HashMap<String, (u64, f64)> = HashMap::new();

    if !quiet {
        println!("\n============================================================");
        println!("     ♠️  Texas Hold'em Multi-Archetype Gauntlet Suite  ♥️     ");
        println!("============================================================");
        println!(
            "Deals per Archetype : {} (Duplicate Mode, Stack={} BB, Seed={:?})",
            deals_per_archetype,
            stack_size / BIG_BLIND,
            base_seed
        );
        if !disabled_guards.is_empty() {
            println!("Disabled Guards     : {:?}", disabled_guards);
        }
        println!(
            "{:<18} | {:>12} | {:>22} | {:>10} | {:>10}",
            "Archetype", "Win Rate", "95% CI (bb/100)", "SB bb/100", "BB bb/100"
        );
        println!(
            "------------------------------------------------------------------------------------"
        );
    }

    let mut total_bb100_sum = 0.0;
    for (idx, &arch) in archetypes.iter().enumerate() {
        let arch_seed = base_seed.map(|s| s.wrapping_add((idx as u64 + 1) * 10_007));
        let res = run_holdem_benchmark_with_stack(
            strategy_path,
            deals_per_archetype,
            0,
            enable_subgame_search,
            "0",
            arch,
            None,
            true,
            exploit_factor,
            true,
            arch_seed,
            disabled_guards,
            stack_size,
        );
        total_bb100_sum += res.win_rate_bb100;
        for (k, (cnt, net)) in &res.guard_telemetry {
            let entry = combined_telemetry.entry(k.clone()).or_insert((0, 0.0));
            entry.0 += *cnt;
            entry.1 += *net;
        }
        if !quiet {
            println!(
                "{:<18} | {:>+9.2} bb | [{:>+8.2}, {:>+8.2}] | {:>+10.2} | {:>+10.2}",
                arch.to_uppercase(),
                res.win_rate_bb100,
                res.ci_95_bb100.0,
                res.ci_95_bb100.1,
                res.sb_win_rate_bb100,
                res.bb_win_rate_bb100
            );
        }
        results.push((arch.to_string(), res));
    }

    let avg_bb100 = total_bb100_sum / archetypes.len() as f64;
    let total_hands = deals_per_archetype * 2 * archetypes.len() as u64;
    if !quiet {
        println!(
            "------------------------------------------------------------------------------------"
        );
        println!(
            "{:<18} | {:>+9.2} bb/100 across {} archetypes ({} total hands)",
            "GAUNTLET AVERAGE",
            avg_bb100,
            archetypes.len(),
            total_hands
        );
        if !combined_telemetry.is_empty() {
            print_guard_telemetry(&combined_telemetry, total_hands);
        }
        println!("============================================================\n");
    }

    results
}

/* Automated Guard Ablation Study across the Gauntlet Suite */
pub fn run_holdem_ablation_benchmark(
    strategy_path: &str,
    deals_per_archetype: u64,
    enable_subgame_search: bool,
    exploit_factor: f64,
    base_seed: Option<u64>,
) {
    let seed = base_seed.or(Some(2026));
    let baseline = run_holdem_gauntlet_benchmark_seeded(
        strategy_path,
        deals_per_archetype,
        enable_subgame_search,
        exploit_factor,
        seed,
        &[],
        false,
    );
    let base_avg: f64 =
        baseline.iter().map(|(_, r)| r.win_rate_bb100).sum::<f64>() / baseline.len().max(1) as f64;

    let guard_candidates = [
        "PASSIVE_STATION_BOOST",
        "PF_BUTTON_STEAL",
        "PF_SB_AGGRESSOR_TRAP",
        "PF_DISCIPLINE_GUARD",
        "SHOVE_DEFENSE_FOLD",
        "RANGE_EQUITY_SHOVE_FOLD",
        "POST_POT_CONTROL_CAP",
        "POST_PAIRED_FLUSH_STRAIGHT_CAP",
        "POST_BARREL_FOLD",
    ];

    println!("============================================================");
    println!(
        "     🔬  Guard Ablation Study (Identical Seed={:?})      ",
        seed
    );
    println!("============================================================");
    println!("Baseline Gauntlet Average : {:+.2} bb/100", base_avg);
    println!(
        "{:<32} | {:>14} | {:>18}",
        "Disabled Guard Family", "Ablated bb/100", "Guard Value (Δ bb)"
    );
    println!("--------------------------------------------------------------------");

    for &g in &guard_candidates {
        let disabled = vec![g.to_string()];
        let ablated = run_holdem_gauntlet_benchmark_seeded(
            strategy_path,
            deals_per_archetype,
            enable_subgame_search,
            exploit_factor,
            seed,
            &disabled,
            true,
        );
        let ablated_avg: f64 = ablated.iter().map(|(_, r)| r.win_rate_bb100).sum::<f64>()
            / ablated.len().max(1) as f64;
        let guard_value = base_avg - ablated_avg;
        println!(
            "{:<32} | {:>+11.2} bb | {:>+15.2} bb",
            g, ablated_avg, guard_value
        );
    }
    println!("============================================================\n");
}
