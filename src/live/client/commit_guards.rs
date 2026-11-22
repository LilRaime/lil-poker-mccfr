/*! Live Stack-Risk and Stack-Commitment Execution Guards */
use crate::cfr::abstraction::{
    BUCKET_TOP_PAIR_MIN, BUCKET_WEAK_DRAW_MIN, BUCKET_WEAK_PAIR_MIN, detect_river_nut_blocker,
    postflop_equity_bucket,
};
use crate::live::state::LiveSnapshot;

pub(super) fn apply_live_commit_guards(
    chosen_action: String,
    raw_amount: i32,
    snapshot: &LiveSnapshot,
    eff_chips: i32,
) -> (String, i32) {
    if (chosen_action == "raise" || chosen_action == "bet") && eff_chips > 0 {
        apply_raise_commit_guard(chosen_action, raw_amount, snapshot, eff_chips)
    } else if chosen_action == "call"
        && eff_chips > snapshot.to_call
        && snapshot.to_call > 0
        && !snapshot.board.is_empty()
        && snapshot.board.len() < 5
    {
        apply_postflop_call_commit_guard(chosen_action, raw_amount, snapshot, eff_chips)
    } else {
        (chosen_action, raw_amount)
    }
}

fn has_legal(legal_actions: &[String], tok: &str) -> bool {
    legal_actions.iter().any(|l| l.eq_ignore_ascii_case(tok))
}

fn prefer_all_in_action(
    legal_actions: &[String],
    fallback_action: String,
    fallback_amount: i32,
) -> (String, i32) {
    if has_legal(legal_actions, "all_in") {
        ("all_in".to_string(), 0)
    } else if has_legal(legal_actions, "allin") {
        ("allin".to_string(), 0)
    } else {
        (fallback_action, fallback_amount)
    }
}

fn apply_postflop_air_risk_downgrade(snapshot: &LiveSnapshot, eff_chips: i32) -> (String, i32) {
    if snapshot.to_call == 0 {
        ("check".to_string(), 0)
    } else if snapshot.to_call <= snapshot.big_blind
        || snapshot.to_call * 6 <= snapshot.pot
        || eff_chips <= snapshot.big_blind
    {
        if has_legal(&snapshot.legal_actions, "call") {
            ("call".to_string(), 0)
        } else if has_legal(&snapshot.legal_actions, "all_in")
            || has_legal(&snapshot.legal_actions, "allin")
        {
            prefer_all_in_action(&snapshot.legal_actions, "call".to_string(), 0)
        } else {
            ("fold".to_string(), 0)
        }
    } else {
        ("fold".to_string(), 0)
    }
}

fn apply_raise_commit_guard(
    chosen_action: String,
    raw_amount: i32,
    snapshot: &LiveSnapshot,
    eff_chips: i32,
) -> (String, i32) {
    let max_total = eff_chips + snapshot.my_bet;
    let min_legal_total = if snapshot.current_bet > 0 {
        snapshot.current_bet + snapshot.to_call.max(snapshot.big_blind)
    } else {
        snapshot.big_blind
    };
    let capped = raw_amount.max(min_legal_total).min(max_total);
    let bet_add = (capped - snapshot.my_bet).max(0);
    let remaining_behind = max_total - capped;
    let new_pot = snapshot.pot + bet_add;

    if snapshot.board.is_empty() {
        /* Preflop Stack-Commit Guard:
         * Convert raises that leave <= 25% of the new pot behind, leave <= 3 BB behind,
         * or commit >= 55% of total stack on preflop directly into an all-in shove! */
        let commits_stack = remaining_behind * 4 <= new_pot
            || remaining_behind <= snapshot.big_blind * 3
            || capped * 20 >= max_total * 11;
        return if commits_stack {
            prefer_all_in_action(&snapshot.legal_actions, chosen_action, max_total)
        } else {
            (chosen_action, capped)
        };
    }

    let b_idx = postflop_equity_bucket(&snapshot.hole, &snapshot.board);
    let river_blocker_bluff = snapshot.board.len() >= 5
        && snapshot.to_call == 0
        && detect_river_nut_blocker(&snapshot.hole, &snapshot.board);
    /* Postflop Air Stack-Risk Guard:
     * Never risk >= 35% of remaining stack on a postflop bet/raise with pure air,
     * except genuine River nut-flush blocker bluffs when checking is an option. */
    if b_idx < BUCKET_WEAK_DRAW_MIN && bet_add * 20 >= eff_chips * 7 && !river_blocker_bluff {
        return apply_postflop_air_risk_downgrade(snapshot, eff_chips);
    }

    /* Include top pair+ and flush/OESD+ draws (19..BUCKET_WEAK_PAIR_MIN), excluding weak gutshots (15..=18) */
    let strong_postflop =
        b_idx >= BUCKET_TOP_PAIR_MIN || (19..BUCKET_WEAK_PAIR_MIN).contains(&b_idx);
    let commits_stack = remaining_behind * 4 <= new_pot
        || remaining_behind <= snapshot.big_blind * 3
        || (strong_postflop && remaining_behind * 2 <= new_pot);
    if commits_stack {
        prefer_all_in_action(&snapshot.legal_actions, chosen_action, max_total)
    } else {
        (chosen_action, capped)
    }
}

fn apply_postflop_call_commit_guard(
    chosen_action: String,
    raw_amount: i32,
    snapshot: &LiveSnapshot,
    eff_chips: i32,
) -> (String, i32) {
    /* Postflop Call Stack-Commit Guard:
     * If calling a Flop/Turn raise with a strong hand (bucket >= 45)
     * leaves <= 3 BB and <= 15% of the new pot behind, push all-in immediately! */
    let rem_after_call = eff_chips - snapshot.to_call;
    let new_pot = snapshot.pot + snapshot.to_call;
    let b_idx = postflop_equity_bucket(&snapshot.hole, &snapshot.board);
    if b_idx >= 45 && rem_after_call <= snapshot.big_blind * 3 && rem_after_call * 6 <= new_pot {
        if has_legal(&snapshot.legal_actions, "all_in")
            || has_legal(&snapshot.legal_actions, "allin")
        {
            prefer_all_in_action(&snapshot.legal_actions, chosen_action, raw_amount)
        } else if has_legal(&snapshot.legal_actions, "raise") {
            ("raise".to_string(), eff_chips + snapshot.my_bet)
        } else {
            (chosen_action, raw_amount)
        }
    } else {
        (chosen_action, raw_amount)
    }
}
