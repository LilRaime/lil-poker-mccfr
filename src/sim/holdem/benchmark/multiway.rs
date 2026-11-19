use std::collections::HashMap;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use super::deal_cards_prefix;
use crate::cfr::abstraction::{
    detect_draws, postflop_equity_bucket, preflop_bucket, preflop_strength_tier,
};
use crate::cfr::fallback::StrategyLookupIndex;
use crate::cfr::opponent_model::OpponentTracker;
use crate::cfr::subgame::{SubgameContext, SubgameSolver};
use crate::game::config::*;
use crate::game::holdem::{
    evaluate_7cards, Card, RoundHistory, ALL_IN, CALL_CHECK, FOLD as H_FOLD, RAISE_HALF_POT,
    RAISE_MIN, RAISE_THIRD_POT,
};
use crate::live::decide_action_traced_with_bet;
use crate::sim::stats::SimStats;

/* 6-Max Multi-Way Table Simulation Benchmark (Duplicate Seat Rotation) */
pub fn run_holdem_multiway_benchmark(
    strategy_path: &str,
    total_deals: u64,
    enable_subgame_search: bool,
    exploit_factor: f64,
    seed: Option<u64>,
) -> f64 {
    run_holdem_multiway_benchmark_with_stack(
        strategy_path,
        total_deals,
        enable_subgame_search,
        exploit_factor,
        seed,
        STACK_SIZE,
    )
}

pub fn run_holdem_multiway_benchmark_with_stack(
    strategy_path: &str,
    total_deals: u64,
    enable_subgame_search: bool,
    exploit_factor: f64,
    seed: Option<u64>,
    stack_size: i32,
) -> f64 {
    let stack_size = stack_size.max(BIG_BLIND * 2);
    let strategy = crate::live::load_live_strategy(strategy_path);
    let strategy_index = StrategyLookupIndex::build_from_map(&strategy);
    let subgame_solver = SubgameSolver::default();
    let mut rng = SmallRng::seed_from_u64(seed.unwrap_or(2026));

    const NUM_SEATS: usize = 6;
    let opp_roster = ["tag", "calling_station", "maniac", "shove_bot", "rock"];
    let mut trackers: Vec<OpponentTracker> = (0..opp_roster.len())
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
        "Stack Depth   : {} chips ({} BB)",
        stack_size,
        stack_size / BIG_BLIND
    );
    println!(
        "Total Deals   : {} (6-Seat Duplicate Rotation = {} hands)",
        total_deals,
        total_deals * NUM_SEATS as u64
    );

    const CARDS_NEEDED: usize = NUM_SEATS * 2 + 5;
    for _deal in 1..=total_deals {
        let deck = deal_cards_prefix(&mut rng, CARDS_NEEDED);
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
                stack_size,
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
fn decide_archetype_action(
    arch: &str,
    hole: &[Card; 2],
    board_slice: &[Card],
    round: u8,
    to_call: i32,
    pot: i32,
    raises_this_round: u8,
    rng: &mut SmallRng,
) -> u8 {
    let (idx, _) = preflop_bucket(hole[0], hole[1]);
    let tier = preflop_strength_tier(idx);
    let bucket = if round == 1 {
        (9 - tier as usize) * 8
    } else {
        postflop_equity_bucket(hole, board_slice)
    };
    let (has_fd, has_sd) = if round >= 2 {
        detect_draws(hole, board_slice)
    } else {
        (false, false)
    };

    match arch {
        "calling_station" => {
            if to_call == 0
                || (round == 1 && (tier <= 6 || (tier == 7 && to_call <= BIG_BLIND * 4)))
                || (round >= 2 && (bucket >= 28 || has_fd || has_sd))
                || to_call <= BIG_BLIND * 2
            {
                CALL_CHECK
            } else {
                H_FOLD
            }
        }
        "maniac" => {
            if raises_this_round < MAX_RAISES_PER_ROUND && rng.gen_bool(0.45) {
                RAISE_HALF_POT
            } else if to_call == 0 || bucket >= 20 || rng.gen_bool(0.50) {
                CALL_CHECK
            } else {
                H_FOLD
            }
        }
        "shove_bot" => {
            if raises_this_round < MAX_RAISES_PER_ROUND
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
                if raises_this_round < MAX_RAISES_PER_ROUND {
                    RAISE_THIRD_POT
                } else {
                    CALL_CHECK
                }
            } else if to_call == 0 || (round == 1 && tier <= 2 && to_call <= BIG_BLIND * 2) {
                CALL_CHECK
            } else {
                H_FOLD
            }
        }
        _ => {
            /* TAG */
            if (round == 1 && tier <= 2) || (round >= 2 && bucket >= 45) {
                if raises_this_round < MAX_RAISES_PER_ROUND {
                    RAISE_THIRD_POT
                } else {
                    CALL_CHECK
                }
            } else if to_call == 0
                || (round == 1 && tier <= 4 && to_call <= BIG_BLIND * 3)
                || (round >= 2 && (bucket >= 34 || has_fd || has_sd) && to_call <= pot / 2)
            {
                CALL_CHECK
            } else {
                H_FOLD
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_bot_seat_action(
    bot_seat: usize,
    holes: &[[Card; 2]; 6],
    board_slice: &[Card],
    round: u8,
    to_call: i32,
    pot: i32,
    my_remaining: i32,
    active_now: usize,
    stack_size: i32,
    strategy: &HashMap<String, Vec<f64>>,
    strategy_index: &StrategyLookupIndex,
    subgame_solver: &SubgameSolver,
    enable_subgame_search: bool,
    ctx: &mut SubgameContext,
    trackers: &[OpponentTracker],
    folded: &mut [bool; 6],
    contribs: &mut [i32; 6],
    current_bet: &mut i32,
    street_start_bet: i32,
    raises_this_round: &mut u8,
    history: &mut [RoundHistory; 4],
) {
    let mut legal_strs = vec!["fold".to_string(), "call".to_string()];
    if to_call == 0 {
        legal_strs.push("check".to_string());
    }
    if *raises_this_round < MAX_RAISES_PER_ROUND && my_remaining > to_call {
        legal_strs.push("raise".to_string());
        legal_strs.push("all_in".to_string());
    }
    let primary_opp = (0..6)
        .find(|&s| s != bot_seat && !folded[s])
        .unwrap_or((bot_seat + 1) % 6);
    let primary_opp_idx = if primary_opp < bot_seat {
        primary_opp
    } else {
        primary_opp - 1
    };
    let my_pos = if bot_seat >= 4 { 0 } else { 1 };
    let opp_contrib = contribs[primary_opp];
    let engine_contribs = if my_pos == 0 {
        [contribs[bot_seat], opp_contrib]
    } else {
        [opp_contrib, contribs[bot_seat]]
    };
    let street_current_bet = (*current_bet - street_start_bet).max(0);
    let (act_str, amt, _) = decide_action_traced_with_bet(
        &holes[bot_seat],
        board_slice,
        &legal_strs,
        to_call,
        pot,
        strategy,
        enable_subgame_search,
        &trackers[primary_opp_idx],
        history,
        my_pos,
        engine_contribs,
        *raises_this_round,
        Some(strategy_index),
        Some(subgame_solver),
        Some(ctx),
        active_now.saturating_sub(1).max(1),
        BIG_BLIND,
        my_remaining,
        street_current_bet,
    );

    match act_str.as_str() {
        "fold" => {
            if to_call > 0 {
                folded[bot_seat] = true;
            } else {
                history[(round - 1) as usize].push(CALL_CHECK);
            }
        }
        "all_in" | "allin" => {
            contribs[bot_seat] = stack_size;
            if contribs[bot_seat] > *current_bet {
                *current_bet = contribs[bot_seat];
                *raises_this_round = raises_this_round.saturating_add(1);
            }
            history[(round - 1) as usize].push(ALL_IN);
        }
        "raise" | "bet" => {
            let target = (street_start_bet + amt)
                .max(*current_bet + BIG_BLIND)
                .min(stack_size);
            let bet_delta = (target - contribs[bot_seat]).max(0);
            contribs[bot_seat] = target;
            if target > *current_bet {
                *current_bet = target;
                *raises_this_round = raises_this_round.saturating_add(1);
            }
            let hist_act = if bet_delta >= my_remaining {
                ALL_IN
            } else if bet_delta >= (pot / 2).max(BIG_BLIND) {
                RAISE_HALF_POT
            } else if bet_delta >= (pot / 3).max(BIG_BLIND) {
                RAISE_THIRD_POT
            } else {
                RAISE_MIN
            };
            history[(round - 1) as usize].push(hist_act);
        }
        _ => {
            contribs[bot_seat] = (contribs[bot_seat] + to_call).min(stack_size);
            history[(round - 1) as usize].push(CALL_CHECK);
        }
    }
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
    stack_size: i32,
) -> f64 {
    let mut folded = [false; 6];
    let mut contribs = [0i32; 6];
    contribs[0] = SMALL_BLIND;
    contribs[1] = BIG_BLIND;
    let mut current_bet = BIG_BLIND;
    let mut history = [RoundHistory::new(); 4];
    let mut ctx = SubgameContext::new();
    let mut opp_acted_pf = [false; 5];
    let mut opp_pf_vpip = [false; 5];
    let mut opp_pf_pfr = [false; 5];

    for round in 1..=4u8 {
        let active_count = folded.iter().filter(|&&f| !f).count();
        if active_count <= 1 {
            break;
        }
        let non_allin_count = (0..6)
            .filter(|&s| !folded[s] && contribs[s] < stack_size)
            .count();
        if non_allin_count <= 1 {
            continue;
        }
        let board_slice: &[Card] = match round {
            1 => &[],
            2 => &full_board[0..3],
            3 => &full_board[0..4],
            _ => &full_board[0..5],
        };
        let street_start_bet = if round == 1 { 0 } else { current_bet };
        let mut raises_this_round = 0u8;
        let mut needs_action = [false; 6];
        for seat in 0..6 {
            if !folded[seat] && contribs[seat] < stack_size {
                needs_action[seat] = true;
            }
        }
        let mut cursor = if round == 1 { 2 } else { 0 };
        let mut safety_steps = 0;

        while needs_action.iter().any(|&n| n) && safety_steps < 36 {
            safety_steps += 1;
            let seat = cursor % 6;
            cursor = (seat + 1) % 6;
            if folded[seat] || contribs[seat] >= stack_size || !needs_action[seat] {
                needs_action[seat] = false;
                continue;
            }
            let active_now = folded.iter().filter(|&&f| !f).count();
            if active_now <= 1 {
                break;
            }
            let to_call = (current_bet - contribs[seat]).max(0);
            let pot: i32 = contribs.iter().sum();
            let my_remaining = (stack_size - contribs[seat]).max(0);
            let prev_current_bet = current_bet;

            if seat == bot_seat {
                apply_bot_seat_action(
                    bot_seat,
                    holes,
                    board_slice,
                    round,
                    to_call,
                    pot,
                    my_remaining,
                    active_now,
                    stack_size,
                    strategy,
                    strategy_index,
                    subgame_solver,
                    enable_subgame_search,
                    &mut ctx,
                    trackers,
                    &mut folded,
                    &mut contribs,
                    &mut current_bet,
                    street_start_bet,
                    &mut raises_this_round,
                    &mut history,
                );
            } else {
                let opp_idx = if seat < bot_seat { seat } else { seat - 1 };
                let tracker_slot = opp_idx % opp_roster.len();
                let act = decide_archetype_action(
                    opp_roster[tracker_slot],
                    &holes[seat],
                    board_slice,
                    round,
                    to_call,
                    pot,
                    raises_this_round,
                    rng,
                );
                let prev_c = contribs[seat];
                match act {
                    H_FOLD => {
                        if to_call > 0 {
                            folded[seat] = true;
                        }
                    }
                    ALL_IN => {
                        contribs[seat] = stack_size;
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
                        let target = (current_bet + add).min(stack_size);
                        contribs[seat] = target;
                        if target > current_bet {
                            current_bet = target;
                            raises_this_round = raises_this_round.saturating_add(1);
                        }
                    }
                    _ => {
                        contribs[seat] = (contribs[seat] + to_call).min(stack_size);
                    }
                }
                let bet_added = (contribs[seat] - prev_c).max(0);
                if round == 1 {
                    opp_acted_pf[tracker_slot] = true;
                    if act >= RAISE_MIN {
                        opp_pf_vpip[tracker_slot] = true;
                        opp_pf_pfr[tracker_slot] = true;
                    } else if act == CALL_CHECK && to_call > 0 {
                        opp_pf_vpip[tracker_slot] = true;
                    }
                }
                trackers[tracker_slot].record_action_street_detailed(
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

            needs_action[seat] = false;
            if current_bet > prev_current_bet {
                for other in 0..6 {
                    if other != seat && !folded[other] && contribs[other] < stack_size {
                        needs_action[other] = true;
                    }
                }
            }
        }
    }

    for (i, t) in trackers.iter_mut().enumerate() {
        if opp_acted_pf[i] {
            let opp_seat = if i < bot_seat { i } else { i + 1 };
            t.record_preflop_positional(opp_seat == 0, opp_pf_vpip[i], opp_pf_pfr[i]);
        }
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
