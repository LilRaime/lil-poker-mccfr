use std::collections::HashMap;

use crate::cfr::opponent_model::{OpponentStyle, OpponentTracker};
use crate::game::config::BIG_BLIND_F64;
use crate::sim::stats::SimStats;

pub(super) fn record_guard_reasons(
    telemetry: &mut HashMap<String, (u64, f64)>,
    reasons: Vec<&'static str>,
    ret: f64,
) {
    for r in reasons {
        let entry = telemetry.entry(r.to_string()).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += ret;
    }
}

pub(super) fn print_guard_telemetry(telemetry: &HashMap<String, (u64, f64)>, total_hands: u64) {
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
        println!("{reason:<32} | {cnt:>9} | {pct:>7.1}% | {avg_bb100:>+13.2} bb");
    }
}

pub(super) fn print_opponent_tracker_summary(opp_tracker: &OpponentTracker) {
    if opp_tracker.total_actions() == 0 {
        return;
    }
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

pub(super) fn print_positional_breakdown(pos_stats: &[SimStats; 2]) {
    if pos_stats[0].hands == 0 || pos_stats[1].hands == 0 {
        return;
    }
    println!("\n--- Positional Breakdown ---");
    for (p, stat) in pos_stats.iter().enumerate() {
        let pos_name = if p == 0 {
            "Player 0 (SB / Button)"
        } else {
            "Player 1 (BB)"
        };
        let p_avg = stat.avg_chips();
        let p_se = stat.se_chips();
        println!("{pos_name}:");
        println!("  Hands Played    : {}", stat.hands);
        println!("  Win Percentage  : {:.1}%", stat.win_pct());
        println!("  Net Profit      : {:+.1} chips", stat.total_chips);
        println!("  Avg Profit/Hand : {p_avg:+.3} chips (±{p_se:.3})");
        println!(
            "  Win Rate        : {:+.2} bb/100",
            (p_avg / BIG_BLIND_F64) * 100.0
        );
    }
}
