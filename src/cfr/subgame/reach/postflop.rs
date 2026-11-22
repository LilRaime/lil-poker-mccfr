use super::ReachContext;
use crate::cfr::abstraction::detect_draws;
use crate::game::config::{ALL_IN, RAISE_HALF_POT, ROUND_RIVER, ROUND_TURN};
use crate::game::holdem::{Card, Rank, evaluate_7cards};

fn effective_category(cand: [Card; 2], street_board: &[Card], board_paired: bool) -> u64 {
    let score = evaluate_7cards(&cand, street_board);
    let cand_hits = cand[0].rank == cand[1].rank
        || street_board
            .iter()
            .any(|b| b.rank == cand[0].rank || b.rank == cand[1].rank);
    if board_paired && !cand_hits {
        0
    } else {
        score >> 32
    }
}

/** 2. Flop Bayesian Likelihood Update (active on Flop once opponent acts, and on Turn/River) */
pub(super) fn flop_likelihood(
    cand: [Card; 2],
    board: &[Card],
    round: u8,
    ctx: &ReachContext,
) -> f64 {
    if board.len() < 3 || (round < ROUND_TURN && !ctx.has_flop_actions) {
        return 1.0;
    }
    let flop_board = &board[..3];
    let f_cat = effective_category(cand, flop_board, ctx.flop_board_paired);
    let (fd, sd) = detect_draws(&cand, flop_board);

    if ctx.opp_raised_flop {
        /* Bet-size aware Flop likelihood: larger sizing polarizes range toward 2P+ and combo draws */
        if f_cat >= 2 {
            1.0 /* Two pair+ */
        } else if f_cat == 1 {
            match ctx.max_opp_raise_flop {
                ALL_IN => 0.25,
                RAISE_HALF_POT => 0.55,
                _ => 0.85,
            }
        } else if fd && sd {
            0.95 /* Combo draw */
        } else if fd || sd {
            match ctx.max_opp_raise_flop {
                ALL_IN => 0.40,
                RAISE_HALF_POT => 0.65,
                _ => 0.75,
            }
        } else {
            match ctx.max_opp_raise_flop {
                ALL_IN => 0.03,
                RAISE_HALF_POT => 0.06,
                _ => 0.08,
            }
        }
    } else if ctx.opp_called_flop {
        /* Opponent check-called on flop: mostly draws, middle pairs, floaters */
        if f_cat >= 3 {
            0.45 /* Sets/Straights/Flushes slowplaying */
        } else if f_cat >= 1 {
            0.95 /* One pair */
        } else if fd || sd {
            0.85 /* Flush/straight draws calling */
        } else {
            0.15 /* Floater air */
        }
    } else if f_cat >= 2 {
        0.30 /* Less likely to check monster */
    } else if f_cat == 1 {
        0.80 /* Marginal made hand */
    } else {
        0.70 /* Air checking */
    }
}

/** 3. Turn Bayesian Likelihood Update (active on Turn once opponent acts, and on River) */
pub(super) fn turn_likelihood(
    cand: [Card; 2],
    board: &[Card],
    round: u8,
    ctx: &ReachContext,
) -> f64 {
    if board.len() < 4 || (round < ROUND_RIVER && !ctx.has_turn_actions) {
        return 1.0;
    }
    let turn_board = &board[..4];
    let t_cat = effective_category(cand, turn_board, ctx.turn_board_paired);
    let (fd, sd) = detect_draws(&cand, turn_board);

    if ctx.opp_raised_turn {
        /* Bet-size aware Turn barrel likelihood */
        if t_cat >= 2 {
            1.0
        } else if t_cat == 1 {
            match ctx.max_opp_raise_turn {
                ALL_IN => 0.15,
                RAISE_HALF_POT => 0.45,
                _ => 0.70,
            }
        } else if fd || sd {
            match ctx.max_opp_raise_turn {
                ALL_IN => 0.30,
                RAISE_HALF_POT => 0.55,
                _ => 0.65,
            }
        } else {
            match ctx.max_opp_raise_turn {
                ALL_IN => 0.02,
                RAISE_HALF_POT => 0.04,
                _ => 0.05,
            }
        }
    } else if ctx.opp_called_turn {
        if t_cat >= 2 {
            if ctx.monotone_suit.is_some() && t_cat >= 5 {
                1.15 /* Made flush on 3-flush turn often flat-calls in position to trap! */
            } else {
                0.80
            }
        } else if t_cat == 1 {
            0.95
        } else if fd || sd {
            0.75
        } else {
            0.08
        }
    } else if t_cat >= 2 {
        0.25
    } else {
        0.75
    }
}

/** 3b. River Bayesian Likelihood Update (active when facing a River bet/raise) */
pub(super) fn river_likelihood(
    cand: [Card; 2],
    board: &[Card],
    round: u8,
    ctx: &ReachContext,
) -> f64 {
    if round < ROUND_RIVER || board.len() < 5 || !ctx.opp_raised_river {
        return 1.0;
    }
    let r_cat = effective_category(cand, &board[..5], ctx.river_board_paired);
    if r_cat >= 2 {
        1.0
    } else if r_cat == 1 {
        match ctx.max_opp_raise_river {
            ALL_IN => 0.12,
            RAISE_HALF_POT => 0.30,
            _ => 0.60,
        }
    } else {
        match ctx.max_opp_raise_river {
            ALL_IN => 0.03,
            RAISE_HALF_POT => 0.06,
            _ => 0.08,
        }
    }
}

/** 4. Monotone Texture Blocker Adjustment */
pub(super) fn monotone_texture_multiplier(cand: [Card; 2], ctx: &ReachContext) -> f64 {
    let Some(mono_suit) = ctx.monotone_suit else {
        return 1.0;
    };
    let c0_mono = cand[0].suit == mono_suit;
    let c1_mono = cand[1].suit == mono_suit;
    let opp_aggressive = ctx.opp_raised_flop || ctx.opp_raised_turn || ctx.opp_raised_river;

    if opp_aggressive {
        if c0_mono && c1_mono {
            1.45 /* Made flush raising */
        } else if c0_mono || c1_mono {
            let mono_card = if c0_mono { cand[0] } else { cand[1] };
            if mono_card.rank == Rank::Ace {
                1.35 /* Nut flush draw / blocker */
            } else if mono_card.rank == Rank::King {
                1.20 /* 2nd nut flush draw */
            } else {
                1.05
            }
        } else {
            /* No flush card and aggressive on monotone board: less likely unless set */
            0.70
        }
    } else if ctx.opp_called_flop || ctx.opp_called_turn {
        if c0_mono || c1_mono {
            1.25 /* Calling with flush draw */
        } else {
            0.80 /* Calling without flush draw */
        }
    } else {
        1.00
    }
}
