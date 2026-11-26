/*! Single-Hand Texas Hold'em Simulation Engine */
use std::thread::sleep;
use std::time::Duration;

use rand::rngs::SmallRng;

use super::strategy::{HoldemStrategy, find_strategy_or_fallback};
use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};
use crate::cfr::fallback::{FallbackRequest, StrategyLookupIndex, get_holdem_fallback_strategy};
use crate::cfr::opponent_model::{
    DecisionTrace, GuardSet, OpponentTracker, PolicyRequest, ShortStackRequest,
};
use crate::cfr::subgame::{SubgameRequest, SubgameSolver};
use crate::game::config::{
    BIG_BLIND, ROUND_FLOP, ROUND_PREFLOP, ROUND_RIVER, ROUND_TURN, SMALL_BLIND, STACK_SIZE,
    SUBGAME_FLOP_TRIGGER_POT,
};
use crate::game::holdem::{
    ALL_IN, CALL_CHECK, FOLD as H_FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT, TexasHoldemGame,
};
use crate::sim::opponents::{OpponentArchetype, sample_opponent_action};

#[derive(Clone, Copy)]
pub struct SimHandConfig<'a> {
    pub my_player: usize,
    pub strategy: Option<&'a HoldemStrategy>,
    pub strategy_index: Option<&'a StrategyLookupIndex>,
    pub opp_archetype: OpponentArchetype,
    pub opp_strategy: Option<&'a HoldemStrategy>,
    pub subgame_solver: Option<&'a SubgameSolver>,
    pub enable_subgame_search: bool,
    pub delay_ms: u64,
    pub show_logs: bool,
    pub label: &'a str,
}

impl Default for SimHandConfig<'_> {
    fn default() -> Self {
        Self {
            my_player: 0,
            strategy: None,
            strategy_index: None,
            opp_archetype: OpponentArchetype::Random,
            opp_strategy: None,
            subgame_solver: None,
            enable_subgame_search: false,
            delay_ms: 0,
            show_logs: false,
            label: "",
        }
    }
}

/** Simulate a single hand to completion */
pub fn simulate_holdem_hand(
    game: TexasHoldemGame,
    cfg: &SimHandConfig<'_>,
    opp_tracker: &mut OpponentTracker,
    rng: &mut SmallRng,
) -> f64 {
    simulate_holdem_hand_traced(game, cfg, opp_tracker, rng).0
}

/** Simulate a single hand to completion and return net chips along with triggered guard reasons */
pub fn simulate_holdem_hand_traced(
    mut game: TexasHoldemGame,
    cfg: &SimHandConfig<'_>,
    opp_tracker: &mut OpponentTracker,
    rng: &mut SmallRng,
) -> (f64, GuardSet) {
    if cfg.show_logs {
        println!("\n--- {} ---", cfg.label);
    }

    let hole = game.hole[cfg.my_player];
    let mut state = SimLoopState {
        initial_chips: game.stack_size,
        hole_str: format!("[{}, {}]", hole[0], hole[1]),
        step_count: 0,
        cumulative_reward: 0.0f64,
        prev_my_contrib: game.contributions[cfg.my_player],
        hand_guard_reasons: GuardSet::EMPTY,
        subgame_ctx: crate::cfr::subgame::SubgameContext::new(),
    };
    let mut pf_state = OppPreflopTrackerState {
        opp_is_sb: cfg.my_player == 1,
        opp_acted_pf: false,
        opp_pf_vpip: false,
        opp_pf_pfr: false,
        opp_faced_3bet_recorded: false,
    };

    while !game.is_terminal() {
        let cp = game.current_player();
        if cp == cfg.my_player {
            step_bot_turn(&mut game, cfg, &mut state, opp_tracker, rng);
        } else if !step_opponent_turn(&mut game, cfg, cp, opp_tracker, &mut pf_state, rng) {
            break;
        }
    }

    finalize_sim_hand_tracking(&game, cfg.my_player, opp_tracker, &pf_state);
    let final_ret = game.get_returns()[cfg.my_player];
    log_sim_terminal_summary(&game, cfg, &mut state, final_ret);

    (final_ret, state.hand_guard_reasons)
}

struct SimLoopState {
    initial_chips: i32,
    hole_str: String,
    step_count: usize,
    cumulative_reward: f64,
    prev_my_contrib: i32,
    hand_guard_reasons: GuardSet,
    subgame_ctx: crate::cfr::subgame::SubgameContext,
}

struct OppPreflopTrackerState {
    opp_is_sb: bool,
    opp_acted_pf: bool,
    opp_pf_vpip: bool,
    opp_pf_pfr: bool,
    opp_faced_3bet_recorded: bool,
}

struct SimBotTurn<'a> {
    legal: &'a [u8],
    current_bucket: usize,
    to_call: i32,
    pot: i32,
    cur_my_chips: i32,
    used_subgame: bool,
}

fn phase_name(round: u8) -> &'static str {
    match round {
        ROUND_PREFLOP => "Preflop",
        ROUND_FLOP => "Flop",
        ROUND_TURN => "Turn",
        ROUND_RIVER => "River",
        _ => "Waiting",
    }
}

fn format_board_str(board: &[crate::game::holdem::Card]) -> String {
    format!(
        "[{}]",
        board
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn sim_action_name(act: u8) -> &'static str {
    match act {
        H_FOLD => "FOLD",
        CALL_CHECK => "CALL_CHECK",
        RAISE_MIN => "RAISE_MIN",
        RAISE_THIRD_POT => "RAISE_THIRD_POT",
        RAISE_HALF_POT => "RAISE_HALF_POT",
        ALL_IN => "ALL_IN",
        _ => "UNKNOWN",
    }
}

fn step_bot_turn(
    game: &mut TexasHoldemGame,
    cfg: &SimHandConfig<'_>,
    state: &mut SimLoopState,
    opp_tracker: &OpponentTracker,
    rng: &mut SmallRng,
) {
    state.step_count += 1;
    let phase_str = phase_name(game.round);
    let board_str = format_board_str(&game.board);
    let legal = game.legal_actions();
    let opp = 1 - cfg.my_player;
    let to_call = (game.contributions[opp] - game.contributions[cfg.my_player]).max(0);
    let pot = game.contributions[0] + game.contributions[1];

    let (raw_probs, used_subgame) =
        compute_bot_raw_probs(game, cfg, opp_tracker, state, &legal, to_call, pot);
    let current_bucket = if game.round == ROUND_PREFLOP {
        preflop_bucket(game.hole[cfg.my_player][0], game.hole[cfg.my_player][1]).0
    } else {
        postflop_equity_bucket(&game.hole[cfg.my_player], &game.board)
    };
    let turn = SimBotTurn {
        legal: &legal,
        current_bucket,
        to_call,
        pot,
        cur_my_chips: (state.initial_chips - game.contributions[cfg.my_player]).max(0),
        used_subgame,
    };

    let (act, trace) =
        select_bot_sim_action(game, cfg.my_player, &raw_probs, &turn, opp_tracker, rng);
    state.hand_guard_reasons |= trace.guard_reasons;

    let act_name = sim_action_name(act);
    *game = game.apply_action(act);

    let post_pot = game.contributions[0] + game.contributions[1];
    let my_chips = (state.initial_chips - game.contributions[cfg.my_player]).max(0);
    let delta_contrib = (game.contributions[cfg.my_player] - state.prev_my_contrib) as f64;
    state.prev_my_contrib = game.contributions[cfg.my_player];

    let step_reward = if game.is_terminal() {
        game.get_returns()[cfg.my_player] / state.initial_chips as f64
    } else {
        -0.0005 * delta_contrib
    };
    state.cumulative_reward += step_reward;

    if cfg.show_logs {
        println!(
            "Step {} | Phase: {} | Cards: {} | Board: {} | Action: {} | Pot: {} | My Chips: {} | Reward: {:+.4}",
            state.step_count,
            phase_str,
            state.hole_str,
            board_str,
            act_name,
            post_pot,
            my_chips,
            step_reward
        );
    }
    if cfg.delay_ms > 0 {
        sleep(Duration::from_millis(cfg.delay_ms));
    }
}

fn compute_bot_raw_probs(
    game: &TexasHoldemGame,
    cfg: &SimHandConfig<'_>,
    opp_tracker: &OpponentTracker,
    state: &mut SimLoopState,
    legal: &[u8],
    to_call: i32,
    pot: i32,
) -> (Vec<f64>, bool) {
    let initial_chips = state.initial_chips;
    let subgame_ctx = &mut state.subgame_ctx;
    let used_subgame = cfg.enable_subgame_search
        && (game.round >= ROUND_TURN
            || (game.round == ROUND_FLOP && pot >= SUBGAME_FLOP_TRIGGER_POT));
    let raw_probs = if used_subgame {
        let obs_vpip = if opp_tracker.total_hands >= 6 || opp_tracker.is_overbet_bluffer() {
            Some(opp_tracker.vpip_ratio())
        } else {
            None
        };
        let obs_pfr = if opp_tracker.is_overbet_bluffer() {
            Some(opp_tracker.pfr_ratio().max(0.50))
        } else if opp_tracker.total_hands >= 6 {
            Some(opp_tracker.pfr_ratio())
        } else {
            None
        };
        let subgame_contribs = if initial_chips != STACK_SIZE {
            let scale = STACK_SIZE as f64 / initial_chips.max(1) as f64;
            [
                ((game.contributions[0] as f64 * scale).round() as i32)
                    .clamp(SMALL_BLIND, STACK_SIZE),
                ((game.contributions[1] as f64 * scale).round() as i32)
                    .clamp(SMALL_BLIND, STACK_SIZE),
            ]
        } else {
            game.contributions
        };
        let default_solver;
        let subgame_solver = if let Some(s) = cfg.subgame_solver {
            s
        } else {
            default_solver = SubgameSolver::default();
            &default_solver
        };
        subgame_solver.solve_with_strategy(
            &SubgameRequest {
                hole: game.hole[cfg.my_player],
                board: &game.board,
                round: game.round,
                my_player: cfg.my_player,
                contributions: subgame_contribs,
                raises_this_round: game.raises_this_round,
                opp_vpip: obs_vpip,
                opp_pfr: obs_pfr,
                opp_aggression: opp_tracker.subgame_bluff_multiplier(),
                opp_profile: opp_tracker.subgame_street_profile(),
            },
            &game.history,
            cfg.strategy,
            Some(subgame_ctx),
        )
    } else if let Some(strat) = cfg.strategy {
        let eff_stack_bb = (initial_chips / BIG_BLIND.max(1)).max(1);
        if let Some(s) = crate::cfr::fallback::lookup_strategy_hierarchical_with_stack(
            |k| strat.get(k),
            &game.hole[cfg.my_player],
            &game.board,
            game.round,
            &game.history,
            eff_stack_bb,
        ) {
            s.to_vec()
        } else if let Some(idx) = cfg.strategy_index {
            idx.get_blended_fallback(&FallbackRequest {
                hole: game.hole[cfg.my_player],
                board: &game.board,
                round: game.round,
                to_call,
                pot,
                legal,
                ..Default::default()
            })
        } else {
            find_strategy_or_fallback(
                strat,
                &game.hole[cfg.my_player],
                &game.board,
                game.round,
                to_call,
                pot,
                legal,
            )
        }
    } else {
        get_holdem_fallback_strategy(&FallbackRequest {
            hole: game.hole[cfg.my_player],
            board: &game.board,
            round: game.round,
            to_call,
            pot,
            legal,
            ..Default::default()
        })
        .to_vec()
    };
    (raw_probs, used_subgame)
}

fn select_bot_sim_action(
    game: &TexasHoldemGame,
    my_player: usize,
    raw_probs: &[f64],
    turn: &SimBotTurn<'_>,
    opp_tracker: &OpponentTracker,
    rng: &mut SmallRng,
) -> (u8, crate::cfr::opponent_model::DecisionTrace) {
    let short_choice = if game.round == ROUND_PREFLOP
        && turn.cur_my_chips > 0
        && turn.cur_my_chips <= BIG_BLIND * 12
    {
        opp_tracker.select_short_stack_preflop_action(&ShortStackRequest {
            bucket: turn.current_bucket,
            to_call: turn.to_call,
            pot: turn.pot,
            my_chips: turn.cur_my_chips,
            legal_actions: turn.legal,
            ..Default::default()
        })
    } else {
        None
    };

    if let Some((short_act, short_reason)) = short_choice {
        let total_p = (turn.pot + turn.to_call).max(1);
        let po_bps =
            ((turn.to_call as f64 / total_p as f64).clamp(0.0, 1.0) * 1000.0).round() as u16;
        (
            short_act,
            DecisionTrace {
                bucket: turn.current_bucket,
                round: ROUND_PREFLOP,
                pot_odds_bps: po_bps,
                raw_top_action: short_act,
                final_action: short_act,
                guard_reasons: GuardSet::from_id(short_reason),
            },
        )
    } else {
        opp_tracker.select_action_traced(
            &PolicyRequest {
                raw_probs,
                legal_actions: turn.legal,
                bucket: turn.current_bucket,
                round: game.round,
                to_call: turn.to_call,
                pot: turn.pot,
                hole: Some(&game.hole[my_player]),
                board: Some(&game.board),
                my_chips: turn.cur_my_chips,
                from_subgame: turn.used_subgame,
                my_player: Some(my_player),
                history: Some(&game.history),
                ..Default::default()
            },
            rng,
        )
    }
}

fn step_opponent_turn(
    game: &mut TexasHoldemGame,
    cfg: &SimHandConfig<'_>,
    cp: usize,
    opp_tracker: &mut OpponentTracker,
    pf_state: &mut OppPreflopTrackerState,
    rng: &mut SmallRng,
) -> bool {
    let legal = game.legal_actions();
    if legal.is_empty() {
        return false;
    }
    let prev_opp_contrib = game.contributions[cp];
    let opp_to_call_before = (game.contributions[1 - cp] - prev_opp_contrib).max(0);
    let prev_pot = game.contributions[0] + game.contributions[1];
    let round_before = game.round;
    let facing_bet = round_before >= ROUND_FLOP && opp_to_call_before > 0;
    let prior_pf_raises = if round_before == ROUND_PREFLOP {
        game.history[0].iter().filter(|&&a| a >= RAISE_MIN).count()
    } else {
        0
    };
    let effective_opp_strategy = if cfg.opp_strategy.is_some() {
        cfg.opp_strategy
    } else if cfg.opp_archetype.is_self_play() {
        cfg.strategy
    } else {
        None
    };
    let act = sample_opponent_action(cfg.opp_archetype, effective_opp_strategy, game, cp, rng);
    if round_before == ROUND_PREFLOP {
        if pf_state.opp_pf_pfr && prior_pf_raises >= 2 && !pf_state.opp_faced_3bet_recorded {
            opp_tracker.record_facing_3bet(act == H_FOLD);
            pf_state.opp_faced_3bet_recorded = true;
        }
        pf_state.opp_acted_pf = true;
        if act >= RAISE_MIN {
            pf_state.opp_pf_vpip = true;
            pf_state.opp_pf_pfr = true;
        } else if act == CALL_CHECK && opp_to_call_before > 0 {
            pf_state.opp_pf_vpip = true;
        }
    }
    *game = game.apply_action(act);
    let bet_added = (game.contributions[cp] - prev_opp_contrib).max(0);
    opp_tracker.record_action_street_detailed(act, round_before, facing_bet, bet_added, prev_pot);
    true
}

fn finalize_sim_hand_tracking(
    game: &TexasHoldemGame,
    my_player: usize,
    opp_tracker: &mut OpponentTracker,
    pf_state: &OppPreflopTrackerState,
) {
    if pf_state.opp_acted_pf {
        opp_tracker.record_preflop_positional(
            pf_state.opp_is_sb,
            pf_state.opp_pf_vpip,
            pf_state.opp_pf_pfr,
        );
    }
    if !game.board.is_empty() && game.history.iter().all(|rh| !rh.contains(&H_FOLD)) {
        let opp_player = 1 - my_player;
        let opp_raised_river = game.history[3]
            .iter()
            .enumerate()
            .any(|(idx, &a)| (1 - idx % 2) == opp_player && a >= 2);
        opp_tracker.record_showdown_hand(game.hole[opp_player], &game.board, opp_raised_river);
    }
    opp_tracker.end_hand();
}

fn log_sim_terminal_summary(
    game: &TexasHoldemGame,
    cfg: &SimHandConfig<'_>,
    state: &mut SimLoopState,
    final_ret: f64,
) {
    if cfg.show_logs
        && (state.step_count == 0 || state.prev_my_contrib != game.contributions[cfg.my_player])
    {
        state.step_count += 1;
        let board_str = format_board_str(&game.board);
        let my_chips = (state.initial_chips + final_ret as i32).max(0);
        let term_reward = final_ret / state.initial_chips as f64;
        state.cumulative_reward += term_reward;

        let act_name = if state.step_count == 1
            && game.returns[1 - cfg.my_player] < 0.0
            && game.board.is_empty()
        {
            "OPP_FOLD"
        } else if state.prev_my_contrib != game.contributions[cfg.my_player] {
            "FOLD"
        } else {
            "SHOWDOWN"
        };

        println!(
            "Step {} | Phase: End | Cards: {} | Board: {} | Action: {} | Pot: {} | My Chips: {} | Reward: {:+.4}",
            state.step_count, state.hole_str, board_str, act_name, 0, my_chips, term_reward
        );
    }
    if cfg.show_logs {
        println!(
            "Finished {} | Total Steps: {} | Cumulative Reward: {:+.4} | Net Chips: {:+}",
            cfg.label, state.step_count, state.cumulative_reward, final_ret as i32
        );
    }
}
