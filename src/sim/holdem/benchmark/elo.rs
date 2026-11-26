/*! Round-Robin Duplicate Poker Elo Tournament Across Saved Hold'em Models */

use std::path::Path;

use super::HoldemBenchmarkConfig;
use super::hu::run_holdem_benchmark_preloaded;
use crate::cfr::fallback::StrategyLookupIndex;
use crate::game::config::{BIG_BLIND, STACK_SIZE};
use crate::sim::holdem::strategy::{HoldemStrategy, load_holdem_strategy};
use crate::sim::opponents::OpponentArchetype;

#[derive(Debug, Clone)]
pub struct EloTournamentConfig<'a> {
    pub model_paths: &'a [String],
    pub deals_per_pair: u64,
    pub enable_subgame_search: bool,
    pub exploit_factor: f64,
    pub base_seed: Option<u64>,
    pub stack_size: i32,
    pub quiet: bool,
}

impl Default for EloTournamentConfig<'_> {
    fn default() -> Self {
        Self {
            model_paths: &[],
            deals_per_pair: 200,
            enable_subgame_search: false,
            exploit_factor: 0.70,
            base_seed: Some(2026),
            stack_size: STACK_SIZE,
            quiet: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EloEntry {
    pub name: String,
    pub path: String,
    pub rating: f64,
    pub avg_bb100: f64,
    pub matches: usize,
}

struct LoadedModel {
    name: String,
    path: String,
    strategy: HoldemStrategy,
    index: StrategyLookupIndex,
}

fn short_model_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

fn compute_pairwise_elo(edges: &[Vec<f64>]) -> Vec<f64> {
    let n = edges.len();
    let mut ratings = vec![1500.0; n];
    if n < 2 {
        return ratings;
    }

    for _ in 0..200 {
        let mut next = ratings.clone();
        for i in 0..n {
            let mut delta_sum = 0.0;
            for j in 0..n {
                if i == j {
                    continue;
                }
                let actual = 1.0 / (1.0 + 10.0_f64.powf(-edges[i][j] / 100.0));
                let expected = 1.0 / (1.0 + 10.0_f64.powf((ratings[j] - ratings[i]) / 400.0));
                delta_sum += 16.0 * (actual - expected);
            }
            next[i] += delta_sum / (n - 1) as f64;
        }
        let mean = next.iter().sum::<f64>() / n as f64;
        for r in &mut next {
            *r += 1500.0 - mean;
        }
        ratings = next;
    }
    ratings
}

/** Run round-robin Duplicate Poker Elo tournament across model binaries in `cfg.model_paths` */
pub fn run_holdem_elo_tournament(cfg: &EloTournamentConfig<'_>) -> Vec<EloEntry> {
    let stack_size = cfg.stack_size.max(BIG_BLIND * 2);
    let mut models: Vec<LoadedModel> = Vec::new();

    for path in cfg.model_paths {
        if let Some(strategy) = load_holdem_strategy(path) {
            let index = StrategyLookupIndex::build_from_map(&strategy);
            models.push(LoadedModel {
                name: short_model_name(path),
                path: path.clone(),
                strategy,
                index,
            });
        } else if !cfg.quiet {
            eprintln!("[elo] Skipping unreadable strategy '{path}'");
        }
    }

    let n = models.len();
    if n < 2 {
        if !cfg.quiet {
            eprintln!(
                "[elo] Need at least 2 valid models to run a round-robin Elo tournament (loaded {n})."
            );
        }
        return models
            .into_iter()
            .map(|m| EloEntry {
                name: m.name,
                path: m.path,
                rating: 1500.0,
                avg_bb100: 0.0,
                matches: 0,
            })
            .collect();
    }

    let mut edges = vec![vec![0.0f64; n]; n];
    if !cfg.quiet {
        println!("\n============================================================");
        println!("     🏆  Hold'em Round-Robin Duplicate Elo Tournament  🏆    ");
        println!("============================================================");
        println!(
            "Models: {} | Deals/Pair: {} ({} hands/pair) | Stack: {} BB",
            n,
            cfg.deals_per_pair,
            cfg.deals_per_pair * 2,
            stack_size / BIG_BLIND
        );
        println!("------------------------------------------------------------");
    }

    for i in 0..n {
        for j in (i + 1)..n {
            let pair_seed = cfg
                .base_seed
                .map(|s| s.wrapping_add(((i * n + j + 1) as u64) * 10_007));
            let res = run_holdem_benchmark_preloaded(
                &HoldemBenchmarkConfig {
                    strategy_path: &models[i].path,
                    total_episodes: cfg.deals_per_pair,
                    enable_subgame_search: cfg.enable_subgame_search,
                    opp_archetype: OpponentArchetype::SelfPlay,
                    opp_strategy_path: Some(&models[j].path),
                    duplicate_mode: true,
                    exploit_factor: cfg.exploit_factor,
                    quiet: true,
                    seed: pair_seed,
                    stack_size,
                    ..Default::default()
                },
                Some(&models[i].strategy),
                Some(&models[i].index),
                Some(&models[j].strategy),
            );
            edges[i][j] = res.win_rate_bb100;
            edges[j][i] = -res.win_rate_bb100;
            if !cfg.quiet {
                println!(
                    "  {:<30} vs {:<30} : {:>+7.2} bb/100",
                    models[i].name, models[j].name, res.win_rate_bb100
                );
            }
        }
    }

    let ratings = compute_pairwise_elo(&edges);
    let mut entries: Vec<EloEntry> = (0..n)
        .map(|i| {
            let sum_edge: f64 = edges[i].iter().sum();
            let avg_bb100 = sum_edge / (n - 1) as f64;
            EloEntry {
                name: models[i].name.clone(),
                path: models[i].path.clone(),
                rating: ratings[i],
                avg_bb100,
                matches: n - 1,
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        b.rating
            .partial_cmp(&a.rating)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if !cfg.quiet {
        println!("\n--- Elo Standings ---");
        println!(
            "{:<4} | {:<34} | {:>8} | {:>12} | {:>7}",
            "Rank", "Model", "Elo", "Avg Edge", "Pairs"
        );
        println!("---------------------------------------------------------------------------");
        for (rank, e) in entries.iter().enumerate() {
            println!(
                "{:<4} | {:<34} | {:>8.1} | {:>+9.2} bb | {:>7}",
                rank + 1,
                e.name,
                e.rating,
                e.avg_bb100,
                e.matches
            );
        }
        println!("============================================================\n");
    }

    entries
}
