/* Single-Hand Texas Hold'em Simulation Engine */
use std::thread::sleep;
use std::time::Duration;

use rand::rngs::SmallRng;

use super::strategy::{find_strategy_or_fallback, HoldemStrategy};
use crate::cfr::abstraction::{postflop_equity_bucket, preflop_bucket};
use crate::cfr::fallback::{get_holdem_fallback_strategy, StrategyLookupIndex};
use crate::cfr::opponent_model::OpponentTracker;
use crate::cfr::subgame::SubgameSolver;
use crate::game::config::*;
use crate::game::holdem::{
    TexasHoldemGame, ALL_IN, CALL_CHECK, FOLD as H_FOLD, RAISE_HALF_POT, RAISE_MIN, RAISE_THIRD_POT,
};
use crate::sim::opponents::sample_opponent_action;

/* Simulate a single hand to completion */
#[allow(clippy::too_many_arguments)]
pub fn simulate_holdem_hand(
    game: TexasHoldemGame,
    my_player: usize,
    strategy: &Option<HoldemStrategy>,
    strategy_index: Option<&StrategyLookupIndex>,
    opp_archetype: &str,
    opp_strategy: &Option<HoldemStrategy>,
    subgame_solver: &SubgameSolver,
    enable_subgame_search: bool,
    opp_tracker: &mut OpponentTracker,
    rng: &mut SmallRng,
    delay_ms: u64,
    show_logs: bool,
    label: &str,
) -> f64 {
    simulate_holdem_hand_traced(
        game,
        my_player,
        strategy,
        strategy_index,
        opp_archetype,
        opp_strategy,
        subgame_solver,
        enable_subgame_search,
        opp_tracker,
        rng,
        delay_ms,
        show_logs,
        label,
    )
    .0
}

/* Simulate a single hand to completion and return net chips along with triggered guard reasons */
#[allow(clippy::too_many_arguments)]
pub fn simulate_holdem_hand_traced(
    mut game: TexasHoldemGame,
    my_player: usize,
    strategy: &Option<HoldemStrategy>,
    strategy_index: Option<&StrategyLookupIndex>,
    opp_archetype: &str,
    opp_strategy: &Option<HoldemStrategy>,
    subgame_solver: &SubgameSolver,
    enable_subgame_search: bool,
    opp_tracker: &mut OpponentTracker,
    rng: &mut SmallRng,
    delay_ms: u64,
    show_logs: bool,
    label: &str,
) -> (f64, Vec<&'static str>) {
    if show_logs {
        println!("\n--- {} ---", label);
    }

    let initial_chips = game.stack_size;
    let hole = game.hole[my_player];
    let card_str = format!("[{}, {}]", hole[0], hole[1]);

    let mut step_count = 0;
    let mut cumulative_reward = 0.0f64;
    let mut prev_my_contrib = game.contributions[my_player];
    let mut hand_guard_reasons: Vec<&'static str> = Vec::new();
    let mut subgame_ctx = crate::cfr::subgame::SubgameContext::new();
    let opp_is_sb = my_player == 1;
    let mut opp_acted_pf = false;
    let mut opp_pf_vpip = false;
    let mut opp_pf_pfr = false;
    let mut opp_faced_3bet_recorded = false;

    while !game.is_terminal() {
        let cp = game.current_player();

        if cp == my_player {
            step_count += 1;
            let phase_str = match game.round {
                1 => "Preflop",
                2 => "Flop",
                3 => "Turn",
                4 => "River",
                _ => "Waiting",
            };

            let board_str = format!(
                "[{}]",
                game.board
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let legal = game.legal_actions();
            let opp = 1 - my_player;
            let to_call = (game.contributions[opp] - game.contributions[my_player]).max(0);
            let pot = game.contributions[0] + game.contributions[1];

            /* 1. Get raw Blueprint/Subgame probabilities */
            let raw_probs = if enable_subgame_search
                && (game.round >= 3 || (game.round == 2 && pot >= SUBGAME_FLOP_TRIGGER_POT))
            {
                let obs_vpip = if opp_tracker.total_hands >= 6 {
                    Some(opp_tracker.vpip_ratio())
                } else {
                    None
                };
                let obs_pfr = if opp_tracker.total_hands >= 6 {
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
                subgame_solver.solve_with_stats_generic(
                    &game.hole[my_player],
                    &game.board,
                    game.round,
                    &game.history,
                    my_player,
                    subgame_contribs,
                    game.raises_this_round,
                    &mut subgame_ctx,
                    strategy.as_ref(),
                    obs_vpip,
                    obs_pfr,
                )
            } else if let Some(ref strat) = strategy {
                if let Some(s) = crate::cfr::fallback::lookup_strategy_hierarchical(
                    |k| strat.get(k),
                    &game.hole[my_player],
                    &game.board,
                    game.round,
                    &game.history,
                ) {
                    s.to_vec()
                } else if let Some(idx) = strategy_index {
                    idx.get_blended_fallback(
                        &game.hole[my_player],
                        &game.board,
                        game.round,
                        to_call,
                        pot,
                        &legal,
                    )
                } else {
                    find_strategy_or_fallback(
                        strat,
                        &game.hole[my_player],
                        &game.board,
                        game.round,
                        to_call,
                        pot,
                        &legal,
                    )
                }
            } else {
                get_holdem_fallback_strategy(
                    &game.hole[my_player],
                    &game.board,
                    game.round,
                    to_call,
                    pot,
                    &legal,
                )
                .to_vec()
            };

            /* 2. Adjust using Opponent Tracker with hand context */
            let current_bucket = if game.round == 1 {
                preflop_bucket(game.hole[my_player][0], game.hole[my_player][1]).0
            } else {
                postflop_equity_bucket(&game.hole[my_player], &game.board)
            };

            /* 3. Sample action with <= 12 BB Nash Push/Fold fast-path or full purified selection */
            let cur_my_chips = (initial_chips - game.contributions[my_player]).max(0);
            let short_choice =
                if game.round == 1 && cur_my_chips > 0 && cur_my_chips <= BIG_BLIND * 12 {
                    opp_tracker.select_short_stack_preflop_action(
                        current_bucket,
                        to_call,
                        pot,
                        1,
                        BIG_BLIND,
                        cur_my_chips,
                        &legal,
                    )
                } else {
                    None
                };
            let (act, trace) = if let Some((short_act, short_reason)) = short_choice {
                let total_p = (pot + to_call).max(1);
                let po_bps =
                    ((to_call as f64 / total_p as f64).clamp(0.0, 1.0) * 1000.0).round() as u16;
                (
                    short_act,
                    crate::cfr::opponent_model::DecisionTrace {
                        bucket: current_bucket,
                        round: 1,
                        pot_odds_bps: po_bps,
                        raw_top_action: short_act,
                        final_action: short_act,
                        guard_reasons: vec![short_reason],
                    },
                )
            } else {
                opp_tracker.select_action_purified_traced_with_stack(
                    &raw_probs,
                    &legal,
                    current_bucket,
                    game.round,
                    to_call,
                    pot,
                    Some(&game.hole[my_player]),
                    Some(&game.board),
                    1,
                    BIG_BLIND,
                    cur_my_chips,
                    rng,
                )
            };
            for r in trace.guard_reasons {
                if !hand_guard_reasons.contains(&r) {
                    hand_guard_reasons.push(r);
                }
            }

            let act_name = match act {
                H_FOLD => "FOLD",
                CALL_CHECK => "CALL_CHECK",
                RAISE_MIN => "RAISE_MIN",
                RAISE_THIRD_POT => "RAISE_THIRD_POT",
                RAISE_HALF_POT => "RAISE_HALF_POT",
                ALL_IN => "ALL_IN",
                _ => "UNKNOWN",
            };

            game = game.apply_action(act);

            let pot = game.contributions[0] + game.contributions[1];
            let my_chips = (initial_chips - game.contributions[my_player]).max(0);

            let delta_contrib = (game.contributions[my_player] - prev_my_contrib) as f64;
            prev_my_contrib = game.contributions[my_player];

            let step_reward = if game.is_terminal() {
                let final_ret = game.get_returns()[my_player];
                final_ret / initial_chips as f64
            } else {
                -0.0005 * delta_contrib
            };
            cumulative_reward += step_reward;

            if show_logs {
                println!(
                    "Step {} | Phase: {} | Cards: {} | Board: {} | Action: {} | Pot: {} | My Chips: {} | Reward: {:+.4}",
                    step_count, phase_str, card_str, board_str, act_name, pot, my_chips, step_reward
                );
            }

            if delay_ms > 0 {
                sleep(Duration::from_millis(delay_ms));
            }
        } else {
            /* Opponent turn */
            let legal = game.legal_actions();
            if legal.is_empty() {
                break;
            }
            let facing_cbet = game.round == 2 && game.raises_this_round > 0;
            let prev_opp_contrib = game.contributions[cp];
            let opp_to_call_before = (game.contributions[1 - cp] - prev_opp_contrib).max(0);
            let prev_pot = game.contributions[0] + game.contributions[1];
            let round_before = game.round;
            let prior_pf_raises = if round_before == 1 {
                game.history[0].iter().filter(|&&a| a >= RAISE_MIN).count()
            } else {
                0
            };
            let act = sample_opponent_action(opp_archetype, opp_strategy, &game, cp, rng);
            if round_before == 1 {
                if opp_pf_pfr && prior_pf_raises >= 2 && !opp_faced_3bet_recorded {
                    opp_tracker.record_facing_3bet(act == H_FOLD);
                    opp_faced_3bet_recorded = true;
                }
                opp_acted_pf = true;
                if act >= RAISE_MIN {
                    opp_pf_vpip = true;
                    opp_pf_pfr = true;
                } else if act == CALL_CHECK && opp_to_call_before > 0 {
                    opp_pf_vpip = true;
                }
            }
            game = game.apply_action(act);
            let bet_added = (game.contributions[cp] - prev_opp_contrib).max(0);
            /* Record opponent action with detailed bet sizing for opponent modeling tracker */
            opp_tracker.record_action_street_detailed(
                act,
                round_before,
                facing_cbet,
                bet_added,
                prev_pot,
            );
        }
    }

    if opp_acted_pf {
        opp_tracker.record_preflop_positional(opp_is_sb, opp_pf_vpip, opp_pf_pfr);
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

    let final_ret = game.get_returns()[my_player];

    if show_logs && (step_count == 0 || prev_my_contrib != game.contributions[my_player]) {
        step_count += 1;
        let board_str = format!(
            "[{}]",
            game.board
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let pot = 0;
        let my_chips = (initial_chips + final_ret as i32).max(0);
        let term_reward = final_ret / initial_chips as f64;
        cumulative_reward += term_reward;

        let act_name =
            if step_count == 1 && game.returns[1 - my_player] < 0.0 && game.board.is_empty() {
                "OPP_FOLD"
            } else if prev_my_contrib != game.contributions[my_player] {
                "FOLD"
            } else {
                "SHOWDOWN"
            };

        println!(
            "Step {} | Phase: End | Cards: {} | Board: {} | Action: {} | Pot: {} | My Chips: {} | Reward: {:+.4}",
            step_count, card_str, board_str, act_name, pot, my_chips, term_reward
        );
    }

    if show_logs {
        println!(
            "Finished {} | Total Steps: {} | Cumulative Reward: {:+.4} | Net Chips: {:+}",
            label, step_count, cumulative_reward, final_ret as i32
        );
    }

    (final_ret, hand_guard_reasons)
}
