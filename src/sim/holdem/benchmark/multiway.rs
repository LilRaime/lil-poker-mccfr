use std::collections::HashMap;

use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

use super::deal_cards_prefix;
use crate::cfr::abstraction::{
    BUCKET_WEAK_PAIR_MIN, detect_draws, postflop_equity_bucket, preflop_bucket,
    preflop_strength_tier,
};
use crate::cfr::fallback::StrategyLookupIndex;
use crate::cfr::opponent_model::OpponentTracker;
use crate::cfr::subgame::{SubgameContext, SubgameSolver};
use crate::game::config::{
    BIG_BLIND, BIG_BLIND_F64, MAX_RAISES_PER_ROUND, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER,
    ROUND_TURN, SMALL_BLIND, STACK_SIZE,
};
use crate::game::holdem::{
    ALL_IN, CALL_CHECK, Card, FOLD as H_FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
    RoundHistory, evaluate_7cards,
};
use crate::live::{LiveDecisionRequest, decide_action_traced};
use crate::sim::stats::SimStats;

const NUM_SEATS: usize = 6;
const CARDS_PER_MULTIWAY_DEAL: usize = NUM_SEATS * 2 + 5;

#[derive(Debug, Clone, Copy)]
pub struct MultiwayBenchmarkConfig<'a> {
    pub strategy_path: &'a str,
    pub total_deals: u64,
    pub enable_subgame_search: bool,
    pub exploit_factor: f64,
    pub seed: Option<u64>,
    pub stack_size: i32,
}

impl Default for MultiwayBenchmarkConfig<'_> {
    fn default() -> Self {
        Self {
            strategy_path: "",
            total_deals: 100,
            enable_subgame_search: false,
            exploit_factor: 0.70,
            seed: Some(2026),
            stack_size: STACK_SIZE,
        }
    }
}

struct MultiwayResources<'a> {
    opp_roster: &'a [&'a str; 5],
    strategy: &'a HashMap<String, Vec<f64>>,
    strategy_index: &'a StrategyLookupIndex,
    subgame_solver: &'a SubgameSolver,
    enable_subgame_search: bool,
    stack_size: i32,
}

struct MultiwayTableState {
    folded: [bool; 6],
    contribs: [i32; 6],
    current_bet: i32,
    raises_this_round: u8,
    history: [RoundHistory; 4],
    opp_acted_pf: [bool; 5],
    opp_pf_vpip: [bool; 5],
    opp_pf_pfr: [bool; 5],
}

struct ArchetypeSpot<'a> {
    hole: &'a [Card; 2],
    board_slice: &'a [Card],
    round: u8,
    to_call: i32,
    pot: i32,
    raises_this_round: u8,
}

struct BotTurnInfo<'a> {
    bot_seat: usize,
    holes: &'a [[Card; 2]; 6],
    board_slice: &'a [Card],
    round: u8,
    to_call: i32,
    pot: i32,
    my_remaining: i32,
    active_now: usize,
    street_start_bet: i32,
}

/** 6-Max Multi-Way Table Simulation Benchmark (Duplicate Seat Rotation) */
pub fn run_holdem_multiway_benchmark(cfg: &MultiwayBenchmarkConfig<'_>) -> f64 {
    let stack_size = cfg.stack_size.max(BIG_BLIND * 2);
    let strategy = crate::live::load_live_strategy(cfg.strategy_path);
    let strategy_index = StrategyLookupIndex::build_from_map(&strategy);
    let subgame_solver = SubgameSolver::default();
    let mut rng = SmallRng::seed_from_u64(cfg.seed.unwrap_or(2026));

    let opp_roster = ["tag", "calling_station", "maniac", "shove_bot", "rock"];
    let mut trackers: Vec<OpponentTracker> = (0..opp_roster.len())
        .map(|_| {
            let mut t = OpponentTracker::new();
            t.set_exploit_factor(cfg.exploit_factor);
            t
        })
        .collect();

    let res = MultiwayResources {
        opp_roster: &opp_roster,
        strategy: &strategy,
        strategy_index: &strategy_index,
        subgame_solver: &subgame_solver,
        enable_subgame_search: cfg.enable_subgame_search,
        stack_size,
    };

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
        cfg.total_deals,
        cfg.total_deals * NUM_SEATS as u64
    );

    for _deal in 1..=cfg.total_deals {
        let deck = deal_cards_prefix(&mut rng, CARDS_PER_MULTIWAY_DEAL);
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
                &res,
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
    println!("Multi-Way Win Rate       : {bb100:+.2} bb/100 (±{se_bb100:.2})");
    println!(
        "95% Confidence Interval  : [{:+.2}, {:+.2}] bb/100",
        bb100 - 1.96 * se_bb100,
        bb100 + 1.96 * se_bb100
    );
    println!("============================================================\n");
    bb100
}

fn decide_archetype_action(arch: &str, spot: &ArchetypeSpot<'_>, rng: &mut SmallRng) -> u8 {
    let (idx, _) = preflop_bucket(spot.hole[0], spot.hole[1]);
    let tier = preflop_strength_tier(idx);
    let bucket = if spot.round == ROUND_PREFLOP {
        (9 - tier as usize) * 8
    } else {
        postflop_equity_bucket(spot.hole, spot.board_slice)
    };
    let (has_fd, has_sd) = if spot.round >= ROUND_FLOP {
        detect_draws(spot.hole, spot.board_slice)
    } else {
        (false, false)
    };

    match arch {
        "calling_station" => {
            if spot.to_call == 0
                || (spot.round == ROUND_PREFLOP
                    && (tier <= 6 || (tier == 7 && spot.to_call <= BIG_BLIND * 4)))
                || (spot.round >= ROUND_FLOP
                    && (bucket >= BUCKET_WEAK_PAIR_MIN || has_fd || has_sd))
                || spot.to_call <= BIG_BLIND * 2
            {
                CALL_CHECK
            } else {
                H_FOLD
            }
        }
        "maniac" => {
            if spot.raises_this_round < MAX_RAISES_PER_ROUND && rng.random_bool(0.45) {
                RAISE_HALF_POT
            } else if spot.to_call == 0 || bucket >= 20 || rng.random_bool(0.50) {
                CALL_CHECK
            } else {
                H_FOLD
            }
        }
        "shove_bot" => {
            if spot.raises_this_round < MAX_RAISES_PER_ROUND
                && ((spot.round == ROUND_PREFLOP && tier <= 4)
                    || bucket >= 36
                    || rng.random_bool(0.20))
            {
                ALL_IN
            } else if spot.to_call == 0 {
                CALL_CHECK
            } else {
                H_FOLD
            }
        }
        "rock" => {
            if (spot.round == ROUND_PREFLOP && tier <= 1)
                || (spot.round >= ROUND_FLOP && bucket >= 54)
            {
                if spot.raises_this_round < MAX_RAISES_PER_ROUND {
                    RAISE_THIRD_POT
                } else {
                    CALL_CHECK
                }
            } else if spot.to_call == 0
                || (spot.round == ROUND_PREFLOP && tier <= 2 && spot.to_call <= BIG_BLIND * 2)
            {
                CALL_CHECK
            } else {
                H_FOLD
            }
        }
        _ => {
            /* TAG */
            if (spot.round == ROUND_PREFLOP && tier <= 2)
                || (spot.round >= ROUND_FLOP && bucket >= 45)
            {
                if spot.raises_this_round < MAX_RAISES_PER_ROUND {
                    RAISE_THIRD_POT
                } else {
                    CALL_CHECK
                }
            } else if spot.to_call == 0
                || (spot.round == ROUND_PREFLOP && tier <= 4 && spot.to_call <= BIG_BLIND * 3)
                || (spot.round >= ROUND_FLOP
                    && (bucket >= 34 || has_fd || has_sd)
                    && spot.to_call <= spot.pot / 2)
            {
                CALL_CHECK
            } else {
                H_FOLD
            }
        }
    }
}

fn apply_bot_seat_action(
    res: &MultiwayResources<'_>,
    turn: &BotTurnInfo<'_>,
    state: &mut MultiwayTableState,
    ctx: &mut SubgameContext,
    trackers: &[OpponentTracker],
) {
    let mut legal_strs = vec!["fold".to_string(), "call".to_string()];
    if turn.to_call == 0 {
        legal_strs.push("check".to_string());
    }
    if state.raises_this_round < MAX_RAISES_PER_ROUND && turn.my_remaining > turn.to_call {
        legal_strs.push("raise".to_string());
        legal_strs.push("all_in".to_string());
    }
    let primary_opp = (0..6)
        .find(|&s| s != turn.bot_seat && !state.folded[s])
        .unwrap_or((turn.bot_seat + 1) % 6);
    let primary_opp_idx = if primary_opp < turn.bot_seat {
        primary_opp
    } else {
        primary_opp - 1
    };
    let my_pos = if turn.bot_seat >= 4 { 0 } else { 1 };
    let opp_contrib = state.contribs[primary_opp];
    let engine_contribs = if my_pos == 0 {
        [state.contribs[turn.bot_seat], opp_contrib]
    } else {
        [opp_contrib, state.contribs[turn.bot_seat]]
    };
    let street_current_bet = (state.current_bet - turn.street_start_bet).max(0);
    let (act_str, amt, _) = decide_action_traced(
        &LiveDecisionRequest {
            hole: turn.holes[turn.bot_seat],
            board: turn.board_slice,
            legal: &legal_strs,
            to_call: turn.to_call,
            pot: turn.pot,
            subgame_search: res.enable_subgame_search,
            history: state.history,
            my_player: my_pos,
            contributions: engine_contribs,
            raises_this_round: state.raises_this_round,
            active_opponents: turn.active_now.saturating_sub(1).max(1),
            big_blind: BIG_BLIND,
            my_chips: turn.my_remaining,
            opp_chips: None,
            current_bet: street_current_bet,
            strategy_index: Some(res.strategy_index),
            solver: Some(res.subgame_solver),
            ..Default::default()
        },
        res.strategy,
        &trackers[primary_opp_idx],
        Some(ctx),
    );

    let r_idx = (turn.round - 1) as usize;
    match act_str.as_str() {
        "fold" => {
            if turn.to_call > 0 {
                state.folded[turn.bot_seat] = true;
            } else {
                state.history[r_idx].push(CALL_CHECK);
            }
        }
        "all_in" | "allin" => {
            state.contribs[turn.bot_seat] = res.stack_size;
            if state.contribs[turn.bot_seat] > state.current_bet {
                state.current_bet = state.contribs[turn.bot_seat];
                state.raises_this_round = state.raises_this_round.saturating_add(1);
            }
            state.history[r_idx].push(ALL_IN);
        }
        "raise" | "bet" => {
            let target = (turn.street_start_bet + amt)
                .max(state.current_bet + BIG_BLIND)
                .min(res.stack_size);
            let bet_delta = (target - state.contribs[turn.bot_seat]).max(0);
            state.contribs[turn.bot_seat] = target;
            if target > state.current_bet {
                state.current_bet = target;
                state.raises_this_round = state.raises_this_round.saturating_add(1);
            }
            let hist_act = if bet_delta >= turn.my_remaining {
                ALL_IN
            } else if bet_delta >= (turn.pot / 2).max(BIG_BLIND) {
                RAISE_HALF_POT
            } else if bet_delta >= (turn.pot / 3).max(BIG_BLIND) {
                RAISE_THIRD_POT
            } else {
                RAISE_MIN
            };
            state.history[r_idx].push(hist_act);
        }
        _ => {
            state.contribs[turn.bot_seat] =
                (state.contribs[turn.bot_seat] + turn.to_call).min(res.stack_size);
            state.history[r_idx].push(CALL_CHECK);
        }
    }
}

fn step_multiway_opp_seat(
    seat: usize,
    bot_seat: usize,
    spot: &ArchetypeSpot<'_>,
    res: &MultiwayResources<'_>,
    state: &mut MultiwayTableState,
    trackers: &mut [OpponentTracker],
    rng: &mut SmallRng,
) {
    let opp_idx = if seat < bot_seat { seat } else { seat - 1 };
    let tracker_slot = opp_idx % res.opp_roster.len();
    let act = decide_archetype_action(res.opp_roster[tracker_slot], spot, rng);
    let prev_c = state.contribs[seat];
    match act {
        H_FOLD => {
            if spot.to_call > 0 {
                state.folded[seat] = true;
            }
        }
        ALL_IN => {
            state.contribs[seat] = res.stack_size;
            if state.contribs[seat] > state.current_bet {
                state.current_bet = state.contribs[seat];
                state.raises_this_round = state.raises_this_round.saturating_add(1);
            }
        }
        RAISE_HALF_POT | RAISE_THIRD_POT | RAISE_MIN => {
            let add = if act == RAISE_HALF_POT {
                (spot.pot * 2 / 3).max(BIG_BLIND * 2)
            } else {
                (spot.pot / 2).max(BIG_BLIND * 2)
            };
            let target = (state.current_bet + add).min(res.stack_size);
            state.contribs[seat] = target;
            if target > state.current_bet {
                state.current_bet = target;
                state.raises_this_round = state.raises_this_round.saturating_add(1);
            }
        }
        _ => {
            state.contribs[seat] = (state.contribs[seat] + spot.to_call).min(res.stack_size);
        }
    }
    let bet_added = (state.contribs[seat] - prev_c).max(0);
    if spot.round == ROUND_PREFLOP {
        state.opp_acted_pf[tracker_slot] = true;
        if act >= RAISE_MIN {
            state.opp_pf_vpip[tracker_slot] = true;
            state.opp_pf_pfr[tracker_slot] = true;
        } else if act == CALL_CHECK && spot.to_call > 0 {
            state.opp_pf_vpip[tracker_slot] = true;
        }
    }
    trackers[tracker_slot].record_action_street_detailed(
        act,
        spot.round,
        spot.round == ROUND_FLOP && state.raises_this_round > 0,
        bet_added,
        spot.pot,
    );
    if act != H_FOLD {
        state.history[(spot.round - 1) as usize].push(act);
    }
}

struct MultiwayRoundContext<'a> {
    bot_seat: usize,
    holes: &'a [[Card; 2]; 6],
    board_slice: &'a [Card],
    round: u8,
}

fn run_multiway_round(
    rctx: &MultiwayRoundContext<'_>,
    res: &MultiwayResources<'_>,
    state: &mut MultiwayTableState,
    ctx: &mut SubgameContext,
    trackers: &mut [OpponentTracker],
    rng: &mut SmallRng,
) {
    let street_start_bet = if rctx.round == ROUND_PREFLOP {
        0
    } else {
        state.current_bet
    };
    state.raises_this_round = 0;
    let mut needs_action = [false; 6];
    for (seat, flag) in needs_action.iter_mut().enumerate() {
        if !state.folded[seat] && state.contribs[seat] < res.stack_size {
            *flag = true;
        }
    }
    let mut cursor = if rctx.round == ROUND_PREFLOP { 2 } else { 0 };
    let mut safety_steps = 0;

    while needs_action.iter().any(|&n| n) && safety_steps < 36 {
        safety_steps += 1;
        let seat = cursor % 6;
        cursor = (seat + 1) % 6;
        if state.folded[seat] || state.contribs[seat] >= res.stack_size || !needs_action[seat] {
            needs_action[seat] = false;
            continue;
        }
        let active_now = state.folded.iter().filter(|&&f| !f).count();
        if active_now <= 1 {
            break;
        }
        let to_call = (state.current_bet - state.contribs[seat]).max(0);
        let pot: i32 = state.contribs.iter().sum();
        let my_remaining = (res.stack_size - state.contribs[seat]).max(0);
        let prev_current_bet = state.current_bet;

        if seat == rctx.bot_seat {
            let turn = BotTurnInfo {
                bot_seat: rctx.bot_seat,
                holes: rctx.holes,
                board_slice: rctx.board_slice,
                round: rctx.round,
                to_call,
                pot,
                my_remaining,
                active_now,
                street_start_bet,
            };
            apply_bot_seat_action(res, &turn, state, ctx, trackers);
        } else {
            let spot = ArchetypeSpot {
                hole: &rctx.holes[seat],
                board_slice: rctx.board_slice,
                round: rctx.round,
                to_call,
                pot,
                raises_this_round: state.raises_this_round,
            };
            step_multiway_opp_seat(seat, rctx.bot_seat, &spot, res, state, trackers, rng);
        }

        needs_action[seat] = false;
        if state.current_bet > prev_current_bet {
            for other in 0..6 {
                if other != seat && !state.folded[other] && state.contribs[other] < res.stack_size {
                    needs_action[other] = true;
                }
            }
        }
    }
}

fn finalize_multiway_hand(
    bot_seat: usize,
    holes: &[[Card; 2]; 6],
    full_board: &[Card; 5],
    state: &MultiwayTableState,
) -> f64 {
    let total_pot: i32 = state.contribs.iter().sum();
    let survivors: Vec<usize> = (0..6).filter(|&s| !state.folded[s]).collect();
    if survivors.len() == 1 {
        if survivors[0] == bot_seat {
            (total_pot - state.contribs[bot_seat]) as f64
        } else {
            -(state.contribs[bot_seat] as f64)
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
            (total_pot as f64 / winners.len() as f64) - state.contribs[bot_seat] as f64
        } else {
            -(state.contribs[bot_seat] as f64)
        }
    }
}

fn simulate_multiway_6max_hand(
    bot_seat: usize,
    holes: &[[Card; 2]; 6],
    full_board: &[Card; 5],
    res: &MultiwayResources<'_>,
    trackers: &mut [OpponentTracker],
    rng: &mut SmallRng,
) -> f64 {
    let mut state = MultiwayTableState {
        folded: [false; 6],
        contribs: [SMALL_BLIND, BIG_BLIND, 0, 0, 0, 0],
        current_bet: BIG_BLIND,
        raises_this_round: 0,
        history: [RoundHistory::new(); 4],
        opp_acted_pf: [false; 5],
        opp_pf_vpip: [false; 5],
        opp_pf_pfr: [false; 5],
    };
    let mut ctx = SubgameContext::new();

    for round in ROUND_PREFLOP..=ROUND_RIVER {
        if state.folded.iter().filter(|&&f| !f).count() <= 1 {
            break;
        }
        if (0..6)
            .filter(|&s| !state.folded[s] && state.contribs[s] < res.stack_size)
            .count()
            <= 1
        {
            continue;
        }
        let board_slice: &[Card] = match round {
            ROUND_PREFLOP => &[],
            ROUND_FLOP => &full_board[0..3],
            ROUND_TURN => &full_board[0..4],
            _ => &full_board[0..5],
        };
        let rctx = MultiwayRoundContext {
            bot_seat,
            holes,
            board_slice,
            round,
        };
        run_multiway_round(&rctx, res, &mut state, &mut ctx, trackers, rng);
    }

    for (i, t) in trackers.iter_mut().enumerate() {
        if state.opp_acted_pf[i] {
            let opp_seat = if i < bot_seat { i } else { i + 1 };
            t.record_preflop_positional(opp_seat == 0, state.opp_pf_vpip[i], state.opp_pf_pfr[i]);
        }
        t.end_hand();
    }

    finalize_multiway_hand(bot_seat, holes, full_board, &state)
}
