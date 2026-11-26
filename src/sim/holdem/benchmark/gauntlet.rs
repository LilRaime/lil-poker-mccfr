use std::collections::HashMap;

use super::HoldemBenchmarkResult;
use super::hu::{HoldemBenchmarkConfig, run_holdem_benchmark};
use super::telemetry::print_guard_telemetry;
use crate::cfr::opponent_model::{GuardId, GuardSet};
use crate::game::config::{BIG_BLIND, STACK_SIZE};
use crate::sim::opponents::OpponentArchetype;

#[derive(Debug, Clone, Copy)]
pub struct GauntletConfig<'a> {
    pub strategy_path: &'a str,
    pub deals_per_archetype: u64,
    pub enable_subgame_search: bool,
    pub exploit_factor: f64,
    pub base_seed: Option<u64>,
    pub disabled_guards: GuardSet,
    pub quiet: bool,
    pub stack_size: i32,
}

impl Default for GauntletConfig<'_> {
    fn default() -> Self {
        Self {
            strategy_path: "",
            deals_per_archetype: 100,
            enable_subgame_search: false,
            exploit_factor: 0.70,
            base_seed: Some(2026),
            disabled_guards: GuardSet::EMPTY,
            quiet: false,
            stack_size: STACK_SIZE,
        }
    }
}

/** Multi-Archetype Gauntlet Benchmark Suite */
pub fn run_holdem_gauntlet_benchmark(
    cfg: &GauntletConfig<'_>,
) -> Vec<(OpponentArchetype, HoldemBenchmarkResult)> {
    let stack_size = cfg.stack_size.max(BIG_BLIND * 2);
    let archetypes = OpponentArchetype::GAUNTLET_ROSTER;
    let mut results = Vec::with_capacity(archetypes.len());
    let mut combined_telemetry: HashMap<GuardId, (u64, f64)> = HashMap::new();

    if !cfg.quiet {
        println!("\n============================================================");
        println!("     ♠️  Texas Hold'em Multi-Archetype Gauntlet Suite  ♥️     ");
        println!("============================================================");
        println!(
            "Deals per Archetype : {} (Duplicate Mode, Stack={} BB, Seed={:?})",
            cfg.deals_per_archetype,
            stack_size / BIG_BLIND,
            cfg.base_seed
        );
        if !cfg.disabled_guards.is_empty() {
            println!("Disabled Guards     : {:?}", cfg.disabled_guards);
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
        let arch_seed = cfg
            .base_seed
            .map(|s| s.wrapping_add((idx as u64 + 1) * 10_007));
        let res = run_holdem_benchmark(&HoldemBenchmarkConfig {
            strategy_path: cfg.strategy_path,
            total_episodes: cfg.deals_per_archetype,
            enable_subgame_search: cfg.enable_subgame_search,
            opp_archetype: arch,
            exploit_factor: cfg.exploit_factor,
            quiet: true,
            seed: arch_seed,
            disabled_guards: cfg.disabled_guards,
            stack_size,
            ..Default::default()
        });
        total_bb100_sum += res.win_rate_bb100;
        for (&k, &(cnt, net)) in &res.guard_telemetry {
            let entry = combined_telemetry.entry(k).or_insert((0, 0.0));
            entry.0 += cnt;
            entry.1 += net;
        }
        if !cfg.quiet {
            println!(
                "{:<18} | {:>+9.2} bb | [{:>+8.2}, {:>+8.2}] | {:>+10.2} | {:>+10.2}",
                arch.banner_name(),
                res.win_rate_bb100,
                res.ci_95_bb100.0,
                res.ci_95_bb100.1,
                res.sb_win_rate_bb100,
                res.bb_win_rate_bb100
            );
        }
        results.push((arch, res));
    }

    let avg_bb100 = total_bb100_sum / archetypes.len() as f64;
    let total_hands = cfg.deals_per_archetype * 2 * archetypes.len() as u64;
    if !cfg.quiet {
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

/** Automated Guard Ablation Study across the Gauntlet Suite */
pub fn run_holdem_ablation_benchmark(
    strategy_path: &str,
    deals_per_archetype: u64,
    enable_subgame_search: bool,
    exploit_factor: f64,
    base_seed: Option<u64>,
) {
    let seed = base_seed.or(Some(2026));
    let base_cfg = GauntletConfig {
        strategy_path,
        deals_per_archetype,
        enable_subgame_search,
        exploit_factor,
        base_seed: seed,
        ..Default::default()
    };
    let baseline = run_holdem_gauntlet_benchmark(&base_cfg);
    let base_avg: f64 =
        baseline.iter().map(|(_, r)| r.win_rate_bb100).sum::<f64>() / baseline.len().max(1) as f64;

    let extract_arch_bb =
        |res: &[(OpponentArchetype, HoldemBenchmarkResult)], target: OpponentArchetype| -> f64 {
            res.iter()
                .find(|(a, _)| *a == target)
                .map_or(0.0, |(_, r)| r.win_rate_bb100)
        };
    let base_self = extract_arch_bb(&baseline, OpponentArchetype::SelfPlay);
    let base_ob = extract_arch_bb(&baseline, OpponentArchetype::OverbetAggro);

    let guard_candidates = [
        GuardId::PassiveStationBoost,
        GuardId::PfButtonSteal,
        GuardId::PfSbAggressorTrap,
        GuardId::PfDisciplineGuard,
        GuardId::ShoveDefenseFold,
        GuardId::RangeEquityShoveFold,
        GuardId::PostPotControlCap,
        GuardId::PostPairedFlushStraightCap,
        GuardId::PostBarrelFold,
        GuardId::PostRiverBluffCap,
        GuardId::OverbetBluffCatch,
        GuardId::PostOverbetAllinCap,
    ];

    println!("============================================================");
    println!("     🔬  Guard Ablation Study (Identical Seed={seed:?})      ");
    println!("============================================================");
    println!(
        "Baseline Gauntlet Avg: {base_avg:+.2} bb/100 | SELF: {base_self:+.2} | OVERBET_AGGRO: {base_ob:+.2}"
    );
    println!(
        "{:<32} | {:>11} | {:>11} | {:>11} | {:>11}",
        "Disabled Guard Family", "Ablated Avg", "Avg Δ bb", "SELF Δ bb", "OB_AGGRO Δ"
    );
    println!(
        "----------------------------------------------------------------------------------------"
    );

    for &g in &guard_candidates {
        let ablated = run_holdem_gauntlet_benchmark(&GauntletConfig {
            disabled_guards: GuardSet::from_id(g),
            quiet: true,
            ..base_cfg
        });
        let ablated_avg: f64 = ablated.iter().map(|(_, r)| r.win_rate_bb100).sum::<f64>()
            / ablated.len().max(1) as f64;
        let guard_value = base_avg - ablated_avg;
        let self_delta = base_self - extract_arch_bb(&ablated, OpponentArchetype::SelfPlay);
        let ob_delta = base_ob - extract_arch_bb(&ablated, OpponentArchetype::OverbetAggro);
        println!(
            "{:<32} | {ablated_avg:>+8.2} bb | {guard_value:>+8.2} bb | {self_delta:>+8.2} bb | {ob_delta:>+8.2} bb",
            g.as_str()
        );
    }
    println!("============================================================\n");
}
