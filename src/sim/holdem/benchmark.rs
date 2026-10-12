/* Texas Hold'em Benchmark and A/B Evaluation Suite */
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use super::engine::simulate_holdem_hand;
use super::strategy::load_holdem_strategy;
use crate::cfr::fallback::StrategyLookupIndex;
use crate::cfr::opponent_model::{OpponentStyle, OpponentTracker};
use crate::cfr::subgame::SubgameSolver;
use crate::game::config::*;
use crate::game::holdem::{TexasHoldemGame, ALL_52_CARDS};
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

    let mut rng = SmallRng::from_entropy();
    let mut opp_tracker = OpponentTracker::new();
    opp_tracker.set_exploit_factor(exploit_factor);

    let subgame_solver = SubgameSolver::default();

    let mut total_stats = SimStats::default();
    let mut pos_stats = [SimStats::default(), SimStats::default()];
    let mut duplicate_stats = SimStats::default();

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

            // Match 1: Bot is Seat 0 (SB), Opponent is Seat 1 (BB)
            let game1 = TexasHoldemGame::new_dealt(hole_a, hole_b, runout);
            let ret1 = simulate_holdem_hand(
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
                &format!("Deal {}/{} - Match 1 (Bot as SB)", ep, total_episodes),
            );
            pos_stats[0].record(ret1);
            total_stats.record(ret1);

            // Match 2: Bot is Seat 1 (BB), Opponent is Seat 0 (SB) with identical deck
            let game2 = TexasHoldemGame::new_dealt(hole_a, hole_b, runout);
            let ret2 = simulate_holdem_hand(
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
                &format!("Deal {}/{} - Match 2 (Bot as BB)", ep, total_episodes),
            );
            pos_stats[1].record(ret2);
            total_stats.record(ret2);

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
            let ret = simulate_holdem_hand(
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
    if !quiet {
        println!("============================================================\n");
    }

    res
}
