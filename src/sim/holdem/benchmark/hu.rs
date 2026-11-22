use std::collections::HashMap;

use rand::SeedableRng;
use rand::rngs::SmallRng;

use super::super::engine::{SimHandConfig, simulate_holdem_hand_traced};
use super::super::strategy::{HoldemStrategy, load_holdem_strategy};
use super::telemetry::{
    print_guard_telemetry, print_opponent_tracker_summary, print_positional_breakdown,
    record_guard_reasons,
};
use super::{HoldemBenchmarkResult, deal_cards_prefix};
use crate::cfr::fallback::StrategyLookupIndex;
use crate::cfr::opponent_model::OpponentTracker;
use crate::cfr::subgame::SubgameSolver;
use crate::game::config::{
    BIG_BLIND, BIG_BLIND_F64, NUM_COMMUNITY_CARDS, NUM_HOLE_CARDS, NUM_PLAYERS, STACK_SIZE,
};
use crate::game::holdem::TexasHoldemGame;
use crate::sim::stats::SimStats;

const CARDS_PER_DEAL: usize = NUM_PLAYERS * NUM_HOLE_CARDS + NUM_COMMUNITY_CARDS;

#[derive(Debug, Clone, Copy)]
pub struct HoldemBenchmarkConfig<'a> {
    pub strategy_path: &'a str,
    pub total_episodes: u64,
    pub delay_ms: u64,
    pub enable_subgame_search: bool,
    pub player_arg: &'a str,
    pub opp_archetype: &'a str,
    pub opp_strategy_path: Option<&'a str>,
    pub duplicate_mode: bool,
    pub exploit_factor: f64,
    pub quiet: bool,
    pub seed: Option<u64>,
    pub disabled_guards: &'a [String],
    pub stack_size: i32,
}

impl Default for HoldemBenchmarkConfig<'_> {
    fn default() -> Self {
        Self {
            strategy_path: "",
            total_episodes: 100,
            delay_ms: 0,
            enable_subgame_search: false,
            player_arg: "0",
            opp_archetype: "tag",
            opp_strategy_path: None,
            duplicate_mode: true,
            exploit_factor: 0.70,
            quiet: false,
            seed: None,
            disabled_guards: &[],
            stack_size: STACK_SIZE,
        }
    }
}

fn load_strategies_and_log(
    cfg: &HoldemBenchmarkConfig<'_>,
    stack_size: i32,
) -> (Option<HoldemStrategy>, Option<HoldemStrategy>) {
    let strategy = load_holdem_strategy(cfg.strategy_path);
    if !cfg.quiet {
        if strategy.is_some() {
            println!("[holdem] Loaded bot strategy from '{}'", cfg.strategy_path);
        } else {
            println!(
                "[holdem] WARNING: strategy file '{}' not found — bot plays using fallback.",
                cfg.strategy_path
            );
        }
    }

    let opp_strategy = if let Some(path) = cfg.opp_strategy_path {
        let loaded = load_holdem_strategy(path);
        if !cfg.quiet {
            if loaded.is_some() {
                println!("[holdem] Loaded opponent strategy from '{path}'");
            } else {
                eprintln!(
                    "[holdem] WARNING: Strategy file '{}' not found or invalid JSON — opponent falls back to '{}' archetype.",
                    path,
                    cfg.opp_archetype.to_uppercase()
                );
            }
        }
        loaded
    } else if cfg.opp_archetype.eq_ignore_ascii_case("self") {
        strategy.clone()
    } else {
        None
    };

    if !cfg.quiet {
        if let (Some(_), Some(opp_path)) = (&opp_strategy, cfg.opp_strategy_path) {
            println!("\nA/B BENCHMARK: '{}' vs '{}'", cfg.strategy_path, opp_path);
        } else if cfg.opp_archetype.eq_ignore_ascii_case("self") {
            println!("\nSELF-PLAY BENCHMARK: '{}' vs Self", cfg.strategy_path);
        } else {
            println!(
                "[holdem] Opponent archetype: '{}'",
                cfg.opp_archetype.to_uppercase()
            );
        }
        if cfg.duplicate_mode {
            println!(
                "[duplicate] DUPLICATE POKER ACTIVE: Each deal played in reverse seats to eliminate card luck!"
            );
        }
        if stack_size != STACK_SIZE {
            println!(
                "[stack] Effective Stack Depth: {} chips ({} BB)",
                stack_size,
                stack_size / BIG_BLIND
            );
        }
        if cfg.enable_subgame_search {
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
            println!("A/B Matchup           : Bot vs '{opp_path}'");
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
        println!("Skill Edge per Hand   : {edge_per_hand:+.3} chips (±{se_per_hand:.3})");
        println!("Duplicate Win Rate    : {win_rate_bb100:+.2} bb/100 (±{se_bb100:.2})",);
        println!("95% Confidence Interval: [{ci_low:+.2}, {ci_high:+.2}] bb/100");
        println!("Duplicate Win Rate    : {win_rate_mbb:+.1} mbb/hand (±{se_mbb:.1})",);
        if ci_low > 0.0 {
            println!("Statistical Verdict   : ✅ STATISTICALLY SIGNIFICANT +EV (p < 0.05)");
        } else if ci_high < 0.0 {
            println!("Statistical Verdict   : ❌ STATISTICALLY SIGNIFICANT -EV (p < 0.05)");
        } else {
            println!(
                "Statistical Verdict   : ⚪ INCONCLUSIVE (overlaps zero — increase hands for higher statistical power)"
            );
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
        println!("Average Profit      : {avg:+.3} chips/hand (±{se:.3})");
        println!("Win Rate (BB/100)   : {win_rate_bb100:+.2} bb/100 (±{se_bb100:.2})",);
        println!("95% Confidence Interval: [{ci_low:+.2}, {ci_high:+.2}] bb/100");
        println!("Win Rate (mbb/hand) : {win_rate_mbb:+.1} mbb/hand (±{se_mbb:.1})",);
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

struct BenchmarkLoopState<'a> {
    opp_tracker: &'a mut OpponentTracker,
    rng: &'a mut SmallRng,
    total_stats: &'a mut SimStats,
    pos_stats: &'a mut [SimStats; 2],
    duplicate_stats: &'a mut SimStats,
    guard_telemetry: &'a mut HashMap<String, (u64, f64)>,
}

fn run_duplicate_deals(
    cfg: &HoldemBenchmarkConfig<'_>,
    base_hand_cfg: &SimHandConfig<'_>,
    stack_size: i32,
    st: &mut BenchmarkLoopState<'_>,
) {
    let show_logs = !cfg.quiet && (cfg.delay_ms > 0 || cfg.total_episodes <= 10);
    for ep in 1..=cfg.total_episodes {
        let deck = deal_cards_prefix(st.rng, CARDS_PER_DEAL);
        let hole_a = [deck[0], deck[1]];
        let hole_b = [deck[2], deck[3]];
        let runout = &deck[4..CARDS_PER_DEAL];

        if show_logs {
            println!("\n=== Deal {}/{} (Duplicate) ===", ep, cfg.total_episodes);
        }

        let label1 = format!("Deal {}/{} - Seat A (Bot as SB)", ep, cfg.total_episodes);
        let game1 = TexasHoldemGame::new_dealt_with_stack(hole_a, hole_b, runout, stack_size);
        let (ret1, reasons1) = simulate_holdem_hand_traced(
            game1,
            &SimHandConfig {
                my_player: 0,
                show_logs,
                label: &label1,
                ..*base_hand_cfg
            },
            st.opp_tracker,
            st.rng,
        );
        st.pos_stats[0].record(ret1);
        st.total_stats.record(ret1);
        record_guard_reasons(st.guard_telemetry, reasons1, ret1);

        let label2 = format!("Deal {}/{} - Seat B (Bot as BB)", ep, cfg.total_episodes);
        let game2 = TexasHoldemGame::new_dealt_with_stack(hole_a, hole_b, runout, stack_size);
        let (ret2, reasons2) = simulate_holdem_hand_traced(
            game2,
            &SimHandConfig {
                my_player: 1,
                show_logs,
                label: &label2,
                ..*base_hand_cfg
            },
            st.opp_tracker,
            st.rng,
        );
        st.pos_stats[1].record(ret2);
        st.total_stats.record(ret2);
        record_guard_reasons(st.guard_telemetry, reasons2, ret2);

        let net_edge = ret1 + ret2;
        st.duplicate_stats.record(net_edge);

        if !cfg.quiet && (show_logs || ep % 200 == 0 || ep == cfg.total_episodes) {
            println!(
                "Deal {:>4}/{} | SB: {:>+6.1} | BB: {:>+6.1} | Duplicate Edge: {:>+6.1} chips",
                ep, cfg.total_episodes, ret1, ret2, net_edge
            );
        }
    }
}

fn run_standard_episodes(
    cfg: &HoldemBenchmarkConfig<'_>,
    base_hand_cfg: &SimHandConfig<'_>,
    stack_size: i32,
    st: &mut BenchmarkLoopState<'_>,
) {
    for ep in 1..=cfg.total_episodes {
        let my_player = match cfg.player_arg {
            "0" | "sb" => 0usize,
            "1" | "bb" => 1usize,
            _ => ((ep - 1) % 2) as usize,
        };
        let label = format!("Episode {ep}");
        let game = TexasHoldemGame::new_random(st.rng).with_stack(stack_size);
        let (ret, reasons) = simulate_holdem_hand_traced(
            game,
            &SimHandConfig {
                my_player,
                show_logs: !cfg.quiet,
                label: &label,
                ..*base_hand_cfg
            },
            st.opp_tracker,
            st.rng,
        );
        st.pos_stats[my_player].record(ret);
        st.total_stats.record(ret);
        record_guard_reasons(st.guard_telemetry, reasons, ret);
    }
}

/** Full 52-Card Texas Hold'em Benchmark and A/B Evaluation Suite */
pub fn run_holdem_benchmark(cfg: &HoldemBenchmarkConfig<'_>) -> HoldemBenchmarkResult {
    let stack_size = cfg.stack_size.max(BIG_BLIND * 2);
    let (strategy, opp_strategy) = load_strategies_and_log(cfg, stack_size);
    let strategy_index = strategy.as_ref().map(StrategyLookupIndex::build_from_map);

    let mut rng = match cfg.seed {
        Some(s) => SmallRng::seed_from_u64(s),
        None => rand::make_rng(),
    };
    let mut opp_tracker = OpponentTracker::new();
    opp_tracker.set_exploit_factor(cfg.exploit_factor);
    opp_tracker.set_disabled_guards(cfg.disabled_guards.to_vec());
    let subgame_solver = SubgameSolver::default();

    let base_hand_cfg = SimHandConfig {
        my_player: 0,
        strategy: strategy.as_ref(),
        strategy_index: strategy_index.as_ref(),
        opp_archetype: cfg.opp_archetype,
        opp_strategy: opp_strategy.as_ref(),
        subgame_solver: Some(&subgame_solver),
        enable_subgame_search: cfg.enable_subgame_search,
        delay_ms: cfg.delay_ms,
        show_logs: false,
        label: "",
    };

    let mut total_stats = SimStats::default();
    let mut pos_stats = [SimStats::default(), SimStats::default()];
    let mut duplicate_stats = SimStats::default();
    let mut guard_telemetry: HashMap<String, (u64, f64)> = HashMap::new();
    let mut loop_state = BenchmarkLoopState {
        opp_tracker: &mut opp_tracker,
        rng: &mut rng,
        total_stats: &mut total_stats,
        pos_stats: &mut pos_stats,
        duplicate_stats: &mut duplicate_stats,
        guard_telemetry: &mut guard_telemetry,
    };

    if cfg.duplicate_mode {
        run_duplicate_deals(cfg, &base_hand_cfg, stack_size, &mut loop_state);
    } else {
        run_standard_episodes(cfg, &base_hand_cfg, stack_size, &mut loop_state);
    }

    if !cfg.quiet {
        print_opponent_tracker_summary(&opp_tracker);
    }

    let mut res = if cfg.duplicate_mode {
        summarize_duplicate_result(
            &duplicate_stats,
            &total_stats,
            &opp_strategy,
            cfg.opp_strategy_path,
            cfg.opp_archetype,
            cfg.quiet,
        )
    } else {
        summarize_standard_result(&total_stats, cfg.opp_archetype, cfg.quiet)
    };

    if pos_stats[0].hands > 0 {
        res.sb_win_rate_bb100 = (pos_stats[0].avg_chips() / BIG_BLIND_F64) * 100.0;
    }
    if pos_stats[1].hands > 0 {
        res.bb_win_rate_bb100 = (pos_stats[1].avg_chips() / BIG_BLIND_F64) * 100.0;
    }

    if !cfg.quiet {
        print_positional_breakdown(&pos_stats);
        if !guard_telemetry.is_empty() {
            print_guard_telemetry(&guard_telemetry, total_stats.hands.max(1));
        }
        println!("============================================================\n");
    }
    res.guard_telemetry = guard_telemetry;
    res
}
