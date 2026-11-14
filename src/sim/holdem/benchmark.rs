/* Texas Hold'em Benchmark and A/B Evaluation Suite */
use std::collections::HashMap;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use super::engine::simulate_holdem_hand_traced;
use super::strategy::load_holdem_strategy;
use crate::cfr::fallback::StrategyLookupIndex;
use crate::cfr::opponent_model::{OpponentStyle, OpponentTracker};
use crate::cfr::subgame::{SubgameContext, SubgameSolver};
use crate::game::config::*;
use crate::game::holdem::{
    evaluate_7cards, Card, TexasHoldemGame, ALL_52_CARDS, ALL_IN, CALL_CHECK, FOLD as H_FOLD,
    RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use crate::live::decide_action_traced;
use crate::sim::stats::SimStats;

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
    let strategy = load_holdem_strategy(strategy_path);
    let strategy_index = strategy.as_ref().map(StrategyLookupIndex::build_from_map);

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
    } else if opp_archetype.to_lowercase() == "self" {
        strategy.clone()
    } else {
        None
    };

    if !quiet {
        if let (Some(_), Some(opp_path)) = (&opp_strategy, opp_strategy_path) {
            println!("\nA/B BENCHMARK: '{}' vs '{}'", strategy_path, opp_path);
        } else if opp_archetype.to_lowercase() == "self" {
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
        if enable_subgame_search {
            println!("[subgame] Real-Time Search (Subgame Solving) enabled on Turn & River.");
        }
    }

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
            let mut deck = ALL_52_CARDS;
            for i in 0..CARDS_NEEDED {
                let j = rng.gen_range(i..NUM_CARDS_DECK);
                deck.swap(i, j);
            }
            let hole_a = [deck[0], deck[1]];
            let hole_b = [deck[2], deck[3]];
            let runout = &deck[4..CARDS_NEEDED];

            if show_logs {
                println!("\n=== Deal {}/{} (Duplicate) ===", ep, total_episodes);
            }

            /* Seat A: Bot is Seat 0 (SB), Opponent is Seat 1 (BB) */
            let game1 = TexasHoldemGame::new_dealt(hole_a, hole_b, runout);
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
            for r in reasons1 {
                let entry = guard_telemetry.entry(r.to_string()).or_insert((0, 0.0));
                entry.0 += 1;
                entry.1 += ret1;
            }

            /* Seat B: Bot is Seat 1 (BB), Opponent is Seat 0 (SB) with identical deck */
            let game2 = TexasHoldemGame::new_dealt(hole_a, hole_b, runout);
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
            for r in reasons2 {
                let entry = guard_telemetry.entry(r.to_string()).or_insert((0, 0.0));
                entry.0 += 1;
                entry.1 += ret2;
            }

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
            let game = TexasHoldemGame::new_random(&mut rng);
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
            for r in reasons {
                let entry = guard_telemetry.entry(r.to_string()).or_insert((0, 0.0));
                entry.0 += 1;
                entry.1 += ret;
            }
        }
    }

    if !quiet && opp_tracker.total_actions() > 0 {
        println!("\n=== Opponent Modeling Tracker Stats ===");
        println!("Opponent Total Hands   : {}", opp_tracker.total_hands);
        println!(
            "Opponent VPIP          : {:.1}%",
            opp_tracker.vpip_hands as f64 / opp_tracker.total_hands.max(1) as f64 * 100.0
        );
        println!(
            "Opponent PFR           : {:.1}%",
            opp_tracker.pfr_hands as f64 / opp_tracker.total_hands.max(1) as f64 * 100.0
        );
        println!(
            "Opponent Action Mix    : Fold: {:.1}% | Call: {:.1}% | Raise: {:.1}%",
            opp_tracker.fold_ratio() * 100.0,
            opp_tracker.call_ratio() * 100.0,
            opp_tracker.raise_ratio() * 100.0
        );
        if opp_tracker.flop_cbet_faced > 0 {
            println!(
                "Flop Fold to C-bet     : {:.1}% ({}/{})",
                (opp_tracker.flop_fold_to_cbet as f64 / opp_tracker.flop_cbet_faced as f64) * 100.0,
                opp_tracker.flop_fold_to_cbet,
                opp_tracker.flop_cbet_faced,
            );
        }
        if opp_tracker.total_hands > 0 {
            println!(
                "Went to Showdown (WTSD): {:.1}% ({}/{})",
                (opp_tracker.went_to_showdown as f64 / opp_tracker.total_hands as f64) * 100.0,
                opp_tracker.went_to_showdown,
                opp_tracker.total_hands,
            );
        }
        let (classified_style, confidence) = opp_tracker.classify_style();
        if classified_style != OpponentStyle::Unknown {
            println!(
                "Inferred Playstyle     : {} (Confidence: {:.1}%)",
                classified_style,
                confidence * 100.0
            );
        }
        if opp_tracker.showdown_hands > 0 {
            println!(
                "Showdown Analytics     : {} showdowns | Bluffs Caught: {} ({:.1}%) | Traps: {} | Loose: {}",
                opp_tracker.showdown_hands,
                opp_tracker.showdown_bluff_count,
                opp_tracker.showdown_bluff_rate() * 100.0,
                opp_tracker.showdown_trap_count,
                opp_tracker.showdown_loose_count,
            );
        }
    }

    let bb_val = BIG_BLIND_F64;
    let mut res = HoldemBenchmarkResult {
        duplicate_mode,
        ..Default::default()
    };

    if duplicate_mode {
        let edge_per_hand = duplicate_stats.avg_chips() / 2.0;
        let se_per_hand = duplicate_stats.se_chips() / 2.0;
        let win_rate_bb100 = (edge_per_hand / bb_val) * 100.0;
        let se_bb100 = (se_per_hand / bb_val) * 100.0;
        let ci_low = win_rate_bb100 - 1.96 * se_bb100;
        let ci_high = win_rate_bb100 + 1.96 * se_bb100;
        let win_rate_mbb = (edge_per_hand / bb_val) * 1000.0;
        let se_mbb = (se_per_hand / bb_val) * 1000.0;

        res.total_deals = duplicate_stats.hands;
        res.total_hands = total_stats.hands;
        res.net_chips = duplicate_stats.total_chips;
        res.edge_per_hand = edge_per_hand;
        res.se_per_hand = se_per_hand;
        res.win_rate_bb100 = win_rate_bb100;
        res.se_bb100 = se_bb100;
        res.ci_95_bb100 = (ci_low, ci_high);
        res.win_rate_mbb = win_rate_mbb;
        res.se_mbb = se_mbb;
        res.win_pct = duplicate_stats.win_pct();
        res.loss_pct = duplicate_stats.loss_pct();
        res.tie_pct = duplicate_stats.tie_pct();

        if !quiet {
            println!("\n============================================================");
            println!("        ♠️  Texas Hold'em: Duplicate Poker Benchmark  ♥️      ");
            println!("============================================================");
            if let (Some(_), Some(opp_path)) = (&opp_strategy, opp_strategy_path) {
                println!("A/B Matchup           : Bot vs '{}'", opp_path);
            } else if opp_archetype.to_lowercase() == "self" {
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
    } else {
        let avg = total_stats.avg_chips();
        let se = total_stats.se_chips();
        let win_rate_bb100 = (avg / bb_val) * 100.0;
        let se_bb100 = (se / bb_val) * 100.0;
        let ci_low = win_rate_bb100 - 1.96 * se_bb100;
        let ci_high = win_rate_bb100 + 1.96 * se_bb100;

        res.total_hands = total_stats.hands;
        res.net_chips = total_stats.total_chips;
        res.edge_per_hand = avg;
        res.se_per_hand = se;
        res.win_rate_bb100 = win_rate_bb100;
        res.se_bb100 = se_bb100;
        res.ci_95_bb100 = (ci_low, ci_high);
        res.win_rate_mbb = (avg / bb_val) * 1000.0;
        res.se_mbb = (se / bb_val) * 1000.0;
        res.win_pct = total_stats.win_pct();
        res.loss_pct = total_stats.loss_pct();
        res.tie_pct = total_stats.tie_pct();

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
                res.win_rate_mbb, res.se_mbb,
            );
        }
    }

    if pos_stats[0].hands > 0 {
        res.sb_win_rate_bb100 = (pos_stats[0].avg_chips() / bb_val) * 100.0;
    }
    if pos_stats[1].hands > 0 {
        res.bb_win_rate_bb100 = (pos_stats[1].avg_chips() / bb_val) * 100.0;
    }

    if !quiet && pos_stats[0].hands > 0 && pos_stats[1].hands > 0 {
        println!("\n--- Positional Breakdown ---");
        for (p, stat) in pos_stats.iter().enumerate() {
            let pos_name = if p == 0 {
                "Player 0 (SB / Button)"
            } else {
                "Player 1 (BB)"
            };
            let p_avg = stat.avg_chips();
            let p_se = stat.se_chips();
            println!("{}:", pos_name);
            println!("  Hands Played    : {}", stat.hands);
            println!("  Win Percentage  : {:.1}%", stat.win_pct());
            println!("  Net Profit      : {:+.1} chips", stat.total_chips);
            println!("  Avg Profit/Hand : {:+.3} chips (±{:.3})", p_avg, p_se);
            println!(
                "  Win Rate        : {:+.2} bb/100",
                (p_avg / bb_val) * 100.0
            );
        }
    }

    if !quiet && !guard_telemetry.is_empty() {
        print_guard_telemetry(&guard_telemetry, total_stats.hands.max(1));
    }
    res.guard_telemetry = guard_telemetry;

    if !quiet {
        println!("============================================================\n");
    }

    res
}

fn print_guard_telemetry(telemetry: &HashMap<String, (u64, f64)>, total_hands: u64) {
    let mut rows: Vec<(&String, u64, f64)> = telemetry
        .iter()
        .map(|(k, &(cnt, net))| (k, cnt, net))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    println!("\n--- Guard Trigger Telemetry ---");
    println!(
        "{:<32} | {:>9} | {:>8} | {:>16}",
        "Guard Reason Code", "Triggers", "% Hands", "Avg bb/100 (Trig)"
    );
    println!("---------------------------------------------------------------------------");
    for (reason, cnt, net) in rows {
        let pct = (cnt as f64 / total_hands.max(1) as f64) * 100.0;
        let avg_bb100 = ((net / cnt.max(1) as f64) / BIG_BLIND_F64) * 100.0;
        println!(
            "{:<32} | {:>9} | {:>7.1}% | {:>+13.2} bb",
            reason, cnt, pct, avg_bb100
        );
    }
}

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
            "Deals per Archetype : {} (Duplicate Mode, Seed={:?})",
            deals_per_archetype, base_seed
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
        let res = run_holdem_benchmark_seeded(
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

/* 6-Max Multi-Way Table Simulation Benchmark (Duplicate Seat Rotation) */
pub fn run_holdem_multiway_benchmark(
    strategy_path: &str,
    total_deals: u64,
    enable_subgame_search: bool,
    exploit_factor: f64,
    seed: Option<u64>,
) -> f64 {
    let strategy = crate::live::load_live_strategy(strategy_path);
    let strategy_index = StrategyLookupIndex::build_from_map(&strategy);
    let subgame_solver = SubgameSolver::default();
    let mut rng = SmallRng::seed_from_u64(seed.unwrap_or(2026));

    const NUM_SEATS: usize = 6;
    let opp_roster = ["tag", "calling_station", "maniac", "shove_bot", "rock"];
    let mut trackers: Vec<OpponentTracker> = (0..NUM_SEATS)
        .map(|_| {
            let mut t = OpponentTracker::new();
            t.set_exploit_factor(exploit_factor);
            t
        })
        .collect();

    let mut duplicate_stats = SimStats::default();
    let mut total_stats = SimStats::default();

    println!("\n============================================================");
    println!("     ♠️  Texas Hold'em 6-Max Multi-Way Table Benchmark  ♥️    ");
    println!("============================================================");
    println!("Table Lineup  : Bot + TAG + STATION + MANIAC + SHOVE_BOT + ROCK (6-Max)");
    println!(
        "Total Deals   : {} (6-Seat Duplicate Rotation = {} hands)",
        total_deals,
        total_deals * NUM_SEATS as u64
    );

    const CARDS_NEEDED: usize = NUM_SEATS * 2 + 5;
    for _deal in 1..=total_deals {
        let mut deck = ALL_52_CARDS;
        for i in 0..CARDS_NEEDED {
            let j = rng.gen_range(i..NUM_CARDS_DECK);
            deck.swap(i, j);
        }
        let holes: [[Card; 2]; NUM_SEATS] = [
            [deck[0], deck[1]],
            [deck[2], deck[3]],
            [deck[4], deck[5]],
            [deck[6], deck[7]],
            [deck[8], deck[9]],
            [deck[10], deck[11]],
        ];
        let board = [deck[12], deck[13], deck[14], deck[15], deck[16]];

        let mut deal_net = 0.0f64;
        for bot_seat in 0..NUM_SEATS {
            let ret = simulate_multiway_6max_hand(
                bot_seat,
                &holes,
                &board,
                &opp_roster,
                &strategy,
                &strategy_index,
                &subgame_solver,
                enable_subgame_search,
                &mut trackers,
                &mut rng,
            );
            total_stats.record(ret);
            deal_net += ret;
        }
        duplicate_stats.record(deal_net / NUM_SEATS as f64);
    }

    let avg_chips = duplicate_stats.avg_chips();
    let se_chips = duplicate_stats.se_chips();
    let bb100 = (avg_chips / BIG_BLIND_F64) * 100.0;
    let se_bb100 = (se_chips / BIG_BLIND_F64) * 100.0;
    println!("------------------------------------------------------------");
    println!("Total 6-Max Hands Played : {}", total_stats.hands);
    println!(
        "Multi-Way Win Rate       : {:+.2} bb/100 (±{:.2})",
        bb100, se_bb100
    );
    println!(
        "95% Confidence Interval  : [{:+.2}, {:+.2}] bb/100",
        bb100 - 1.96 * se_bb100,
        bb100 + 1.96 * se_bb100
    );
    println!("============================================================\n");
    bb100
}

#[allow(clippy::too_many_arguments)]
fn simulate_multiway_6max_hand(
    bot_seat: usize,
    holes: &[[Card; 2]; 6],
    full_board: &[Card; 5],
    opp_roster: &[&str; 5],
    strategy: &HashMap<String, Vec<f64>>,
    strategy_index: &StrategyLookupIndex,
    subgame_solver: &SubgameSolver,
    enable_subgame_search: bool,
    trackers: &mut [OpponentTracker],
    rng: &mut SmallRng,
) -> f64 {
    use crate::cfr::abstraction::{
        detect_draws, postflop_equity_bucket, preflop_bucket, preflop_strength_tier,
    };

    let mut folded = [false; 6];
    let mut contribs = [0i32; 6];
    contribs[0] = SMALL_BLIND;
    contribs[1] = BIG_BLIND;
    let mut current_bet = BIG_BLIND;
    let mut history = [crate::game::holdem::RoundHistory::new(); 4];
    let mut ctx = SubgameContext::new();

    for round in 1..=4u8 {
        let active_count = folded.iter().filter(|&&f| !f).count();
        if active_count <= 1 {
            break;
        }
        let board_slice: &[Card] = match round {
            1 => &[],
            2 => &full_board[0..3],
            3 => &full_board[0..4],
            _ => &full_board[0..5],
        };
        let mut raises_this_round = 0u8;
        let start_seat = if round == 1 { 2 } else { 0 };

        for pass in 0..2 {
            for offset in 0..6 {
                let seat = (start_seat + offset) % 6;
                if folded[seat] || contribs[seat] >= STACK_SIZE {
                    continue;
                }
                let active_now = folded.iter().filter(|&&f| !f).count();
                if active_now <= 1 {
                    break;
                }
                let to_call = (current_bet - contribs[seat]).max(0);
                if pass > 0 && to_call == 0 {
                    continue;
                }
                let pot: i32 = contribs.iter().sum();
                let my_remaining = (STACK_SIZE - contribs[seat]).max(0);

                if seat == bot_seat {
                    let mut legal_strs = vec!["fold".to_string(), "call".to_string()];
                    if to_call == 0 {
                        legal_strs.push("check".to_string());
                    }
                    if raises_this_round < 3 && my_remaining > to_call {
                        legal_strs.push("raise".to_string());
                        legal_strs.push("all_in".to_string());
                    }
                    let primary_opp = (0..6)
                        .find(|&s| s != bot_seat && !folded[s])
                        .unwrap_or((bot_seat + 1) % 6);
                    let my_pos = if bot_seat >= 4 { 0 } else { 1 };
                    let opp_contrib = contribs[primary_opp];
                    let (act_str, amt, _) = decide_action_traced(
                        &holes[bot_seat],
                        board_slice,
                        &legal_strs,
                        to_call,
                        pot,
                        strategy,
                        enable_subgame_search,
                        &trackers[primary_opp],
                        &history,
                        my_pos,
                        [contribs[bot_seat], opp_contrib],
                        raises_this_round,
                        Some(strategy_index),
                        Some(subgame_solver),
                        Some(&mut ctx),
                        active_now.saturating_sub(1).max(1),
                        BIG_BLIND,
                        my_remaining,
                    );
                    match act_str.as_str() {
                        "fold" => {
                            if to_call > 0 {
                                folded[seat] = true;
                            } else {
                                history[(round - 1) as usize].push(CALL_CHECK);
                            }
                        }
                        "all_in" | "allin" => {
                            contribs[seat] = STACK_SIZE;
                            if contribs[seat] > current_bet {
                                current_bet = contribs[seat];
                                raises_this_round = raises_this_round.saturating_add(1);
                            }
                            history[(round - 1) as usize].push(ALL_IN);
                        }
                        "raise" | "bet" => {
                            let target = amt.max(current_bet + BIG_BLIND).min(STACK_SIZE);
                            contribs[seat] = target;
                            if target > current_bet {
                                current_bet = target;
                                raises_this_round = raises_this_round.saturating_add(1);
                            }
                            history[(round - 1) as usize].push(RAISE_THIRD_POT);
                        }
                        _ => {
                            contribs[seat] = (contribs[seat] + to_call).min(STACK_SIZE);
                            history[(round - 1) as usize].push(CALL_CHECK);
                        }
                    }
                } else {
                    let opp_idx = if seat < bot_seat { seat } else { seat - 1 };
                    let arch = opp_roster[opp_idx % opp_roster.len()];
                    let (idx, _) = preflop_bucket(holes[seat][0], holes[seat][1]);
                    let tier = preflop_strength_tier(idx);
                    let bucket = if round == 1 {
                        (9 - tier as usize) * 8
                    } else {
                        postflop_equity_bucket(&holes[seat], board_slice)
                    };
                    let (has_fd, has_sd) = if round >= 2 {
                        detect_draws(&holes[seat], board_slice)
                    } else {
                        (false, false)
                    };

                    let act = match arch {
                        "calling_station" => {
                            if to_call == 0
                                || bucket >= 28
                                || has_fd
                                || has_sd
                                || to_call <= BIG_BLIND * 2
                            {
                                CALL_CHECK
                            } else {
                                H_FOLD
                            }
                        }
                        "maniac" => {
                            if raises_this_round < 2 && rng.gen_bool(0.45) {
                                RAISE_HALF_POT
                            } else if to_call == 0 || bucket >= 20 || rng.gen_bool(0.50) {
                                CALL_CHECK
                            } else {
                                H_FOLD
                            }
                        }
                        "shove_bot" => {
                            if raises_this_round < 2
                                && ((round == 1 && tier <= 4) || bucket >= 36 || rng.gen_bool(0.20))
                            {
                                ALL_IN
                            } else if to_call == 0 {
                                CALL_CHECK
                            } else {
                                H_FOLD
                            }
                        }
                        "rock" => {
                            if (round == 1 && tier <= 1) || (round >= 2 && bucket >= 54) {
                                if raises_this_round < 2 {
                                    RAISE_THIRD_POT
                                } else {
                                    CALL_CHECK
                                }
                            } else if to_call == 0
                                || (round == 1 && tier <= 2 && to_call <= BIG_BLIND * 2)
                            {
                                CALL_CHECK
                            } else {
                                H_FOLD
                            }
                        }
                        _ => {
                            /* TAG */
                            if (round == 1 && tier <= 2) || (round >= 2 && bucket >= 45) {
                                if raises_this_round < 2 {
                                    RAISE_THIRD_POT
                                } else {
                                    CALL_CHECK
                                }
                            } else if to_call == 0
                                || (round == 1 && tier <= 4 && to_call <= BIG_BLIND * 3)
                                || (round >= 2
                                    && (bucket >= 34 || has_fd || has_sd)
                                    && to_call <= pot / 2)
                            {
                                CALL_CHECK
                            } else {
                                H_FOLD
                            }
                        }
                    };

                    let prev_c = contribs[seat];
                    match act {
                        H_FOLD => {
                            if to_call > 0 {
                                folded[seat] = true;
                            }
                        }
                        ALL_IN => {
                            contribs[seat] = STACK_SIZE;
                            if contribs[seat] > current_bet {
                                current_bet = contribs[seat];
                                raises_this_round = raises_this_round.saturating_add(1);
                            }
                        }
                        RAISE_HALF_POT | RAISE_THIRD_POT | RAISE_MIN => {
                            let add = if act == RAISE_HALF_POT {
                                (pot * 2 / 3).max(BIG_BLIND * 2)
                            } else {
                                (pot / 2).max(BIG_BLIND * 2)
                            };
                            let target = (current_bet + add).min(STACK_SIZE);
                            contribs[seat] = target;
                            if target > current_bet {
                                current_bet = target;
                                raises_this_round = raises_this_round.saturating_add(1);
                            }
                        }
                        _ => {
                            contribs[seat] = (contribs[seat] + to_call).min(STACK_SIZE);
                        }
                    }
                    let bet_added = (contribs[seat] - prev_c).max(0);
                    trackers[seat].record_action_street_detailed(
                        act,
                        round,
                        round == 2 && raises_this_round > 0,
                        bet_added,
                        pot,
                    );
                    if act != H_FOLD {
                        history[(round - 1) as usize].push(act);
                    }
                }
            }
        }
    }

    for t in trackers.iter_mut() {
        t.end_hand();
    }

    let total_pot: i32 = contribs.iter().sum();
    let survivors: Vec<usize> = (0..6).filter(|&s| !folded[s]).collect();
    if survivors.len() == 1 {
        if survivors[0] == bot_seat {
            (total_pot - contribs[bot_seat]) as f64
        } else {
            -(contribs[bot_seat] as f64)
        }
    } else {
        let mut best_score = 0u64;
        let mut winners = Vec::new();
        for &s in &survivors {
            let sc = evaluate_7cards(&holes[s], full_board);
            if sc > best_score {
                best_score = sc;
                winners.clear();
                winners.push(s);
            } else if sc == best_score {
                winners.push(s);
            }
        }
        if winners.contains(&bot_seat) {
            (total_pot as f64 / winners.len() as f64) - contribs[bot_seat] as f64
        } else {
            -(contribs[bot_seat] as f64)
        }
    }
}
