/* Priority Tier 2B: Postflop Hard Raise Caps, Draw Odds & Pot Control */
use super::super::tracker::OpponentTracker;
use crate::cfr::abstraction::{
    detect_board_texture, detect_river_nut_blocker, is_dangerous_board, is_four_flush_board,
    is_four_straight_board,
};
use crate::game::config::*;
use crate::game::holdem::Card;

impl OpponentTracker {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_tier2_postflop_caps_and_pot_control(
        &self,
        probs: &mut [f64],
        legal_actions: &[u8],
        bucket: usize,
        round: u8,
        to_call: i32,
        pot: i32,
        hole: Option<&[Card; 2]>,
        board: Option<&[Card]>,
        active_opponents: usize,
        bb: i32,
        my_chips: i32,
        pot_odds: f64,
        is_three_flush: bool,
        is_passive_station: bool,
        reasons: &mut Vec<&'static str>,
    ) {
        let is_preflop = round == 1;
        if is_preflop {
            return;
        }

        let has_river_nut_blocker = round == 4
            && active_opponents == 1
            && to_call == 0
            && bucket < 28
            && !is_passive_station
            && (self.total_actions() < 6 || self.fold_ratio() >= 0.30)
            && super::river_nut_blocker_allowed(pot, bb, my_chips)
            && hole
                .zip(board)
                .map(|(h, b)| detect_river_nut_blocker(h, b))
                .unwrap_or(false);

        /* Catastrophic air all-in cap: block all-in pushes for pure air (< 15) and non-combo draws (< 25) */
        if bucket < 25
            && !(round == 2 && (22..=27).contains(&bucket))
            && self.is_guard_enabled("POST_ALLIN_BLUFF_CAP")
        {
            if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                let removed = probs[ai_i];
                if removed > 0.0 {
                    probs[ai_i] = 0.0;
                    if let Some(t_i) = legal_actions
                        .iter()
                        .position(|&a| a == RAISE_HALF_POT)
                        .or_else(|| legal_actions.iter().position(|&a| a == RAISE_THIRD_POT))
                        .or_else(|| legal_actions.iter().position(|&a| a == CALL_CHECK))
                    {
                        probs[t_i] += removed;
                    }
                }
            }
        }

        if (bucket < 15 || bucket == 28 || bucket == 29)
            && self.is_guard_enabled("POST_AIR_GUTSHOT_GUARD")
        {
            if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                probs[ai_i] = 0.0;
            }
            if to_call > 0 && (bucket < 15 || bucket == 28) {
                let mut shifted_raise = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted_raise += probs[i];
                        probs[i] = 0.0;
                    }
                }
                let overpriced_gutshot_or_air =
                    (round < 4 && to_call >= bb * 2 && pot_odds >= 0.25)
                        || (round == 3 && to_call >= bb && pot_odds >= 0.16 && bucket < 15)
                        || (round < 4
                            && to_call >= bb
                            && pot_odds > 0.12
                            && (bucket < 11 || bucket == 28));
                if overpriced_gutshot_or_air {
                    if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        reasons.push("POST_AIR_GUTSHOT_GUARD");
                        for p in probs.iter_mut() {
                            *p = 0.0;
                        }
                        probs[f_i] = 1.0;
                    }
                } else if shifted_raise > 0.0 {
                    if pot_odds <= 0.16 {
                        if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                            probs[c_i] += shifted_raise;
                        }
                    } else if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                        probs[f_i] += shifted_raise;
                    }
                }
            }
        }

        /* Turn Draw Direct Pot-Odds Guard */
        if round == 3
            && to_call >= bb * 2
            && (15..=27).contains(&bucket)
            && self.is_guard_enabled("TURN_DRAW_ODDS_FOLD")
        {
            let max_turn_draw_odds = if bucket >= 26 {
                0.40
            } else if bucket >= 24 {
                0.31
            } else if bucket >= 19 {
                0.22
            } else {
                0.19
            };
            if pot_odds > max_turn_draw_odds {
                if let Some(f_i) = legal_actions.iter().position(|&a| a == FOLD) {
                    reasons.push("TURN_DRAW_ODDS_FOLD");
                    for p in probs.iter_mut() {
                        *p = 0.0;
                    }
                    probs[f_i] = 1.0;
                }
            }
        }

        if round == 2
            && to_call == 0
            && ((bucket < 15 && pot >= bb * 8) || ((28..=37).contains(&bucket) && pot >= bb * 10))
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        }

        let is_four_board_danger = board
            .map(|b| is_four_straight_board(b) || is_four_flush_board(b))
            .unwrap_or(false);
        if round >= 3
            && to_call == 0
            && is_four_board_danger
            && bucket < 73
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        } else if round >= 3
            && to_call == 0
            && (bucket < 15 || (round == 4 && bucket < 28))
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            if let Some(hp_i) = legal_actions.iter().position(|&a| a == RAISE_HALF_POT) {
                probs[hp_i] = 0.0;
            }
            let air_pot_cap = if bucket < 11 { bb * 8 } else { bb * 18 };
            let should_cap_bluff = (is_passive_station
                || active_opponents >= 2
                || pot >= air_pot_cap
                || (is_three_flush && bucket < 15))
                && !has_river_nut_blocker;
            if should_cap_bluff {
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] = (probs[c_i] + shifted).max(0.95);
                }
            }
        } else if round == 3
            && to_call == 0
            && (15..=27).contains(&bucket)
            && (is_passive_station || active_opponents >= 2)
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            /* Semi-bluff draws freely HU vs non-stations; only cap all-in/half-pot vs stations or multiway */
            if let Some(ai_i) = legal_actions.iter().position(|&a| a == ALL_IN) {
                probs[ai_i] = 0.0;
            }
            if is_passive_station && is_three_flush {
                let mut shifted = 0.0f64;
                for (i, &a) in legal_actions.iter().enumerate() {
                    if a >= RAISE_MIN {
                        shifted += probs[i];
                        probs[i] = 0.0;
                    }
                }
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] = (probs[c_i] + shifted).max(0.90);
                }
            }
        } else if round >= 3
            && to_call == 0
            && pot >= bb * 8
            && (28..=38).contains(&bucket)
            && !(is_passive_station && bucket >= 35 && !is_three_flush)
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.95);
            }
        }

        /* Pair + Strong Draw Defense on Flop & Turn (bucket 33 or 38) */
        if round < 4
            && to_call > 0
            && (bucket == 33 || bucket == 38)
            && pot_odds <= 0.35
            && !is_three_flush
        {
            if let (Some(f_i), Some(c_i)) = (
                legal_actions.iter().position(|&a| a == FOLD),
                legal_actions.iter().position(|&a| a == CALL_CHECK),
            ) {
                let shift = probs[f_i] * 0.85;
                probs[f_i] -= shift;
                probs[c_i] = (probs[c_i] + shift).max(0.80);
            }
        }

        /* Flop facing a significant bet:
         * - Bare One Pair / Top Pair (28..=47) vs >= 2 BB & >= 24% pot odds -> call only (pot control)
         * - Bare OESD / non-nut Flush Draw (15..=21) vs large bet (>= 4 BB or >= 30% pot odds) -> call, no check-raise */
        if round == 2
            && (((28..=47).contains(&bucket) && to_call >= bb * 2 && pot_odds >= 0.24)
                || ((15..=21).contains(&bucket)
                    && to_call > 0
                    && (to_call >= bb * 4 || pot_odds >= 0.30)))
            && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] += shifted;
            }
        }

        /* Turn facing a bet: never probe-raise or check-raise with weak or marginal hands (bucket < 61) */
        if round == 3 && to_call > 0 && bucket < 61 && self.is_guard_enabled("POST_POT_CONTROL_CAP")
        {
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if shifted > 0.0 {
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] += shifted;
                }
            }
        }

        /* Idiot-end / Board Straight and River Overbet safety */
        if round >= 3 && bucket == 73 && self.is_guard_enabled("POST_PAIRED_FLUSH_STRAIGHT_CAP") {
            reasons.push("POST_PAIRED_FLUSH_STRAIGHT_CAP");
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a == RAISE_HALF_POT || a == ALL_IN || (a >= RAISE_MIN && to_call > 0) {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if to_call == 0 && round == 3 {
                if let Some(r_t) = legal_actions
                    .iter()
                    .position(|&a| a == RAISE_THIRD_POT)
                    .or_else(|| legal_actions.iter().position(|&a| a == RAISE_MIN))
                {
                    probs[r_t] += shifted * 0.75;
                }
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] += shifted * 0.25;
                }
            } else if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.55);
            }
        } else if round >= 3
            && bucket == 74
            && (to_call >= bb * 3 || pot + to_call >= bb * 12 || is_three_flush)
            && self.is_guard_enabled("POST_PAIRED_FLUSH_STRAIGHT_CAP")
        {
            reasons.push("POST_PAIRED_FLUSH_STRAIGHT_CAP");
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a == ALL_IN
                    || (a == RAISE_HALF_POT && (to_call > 0 || is_three_flush))
                    || (a >= RAISE_MIN && to_call >= bb * 3)
                {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.60);
            }
        } else if round == 4
            && (74..=78).contains(&bucket)
            && (is_three_flush
                || board
                    .map(|b| detect_board_texture(b).is_paired || is_four_flush_board(b))
                    .unwrap_or(false))
            && self.is_guard_enabled("POST_PAIRED_FLUSH_STRAIGHT_CAP")
        {
            reasons.push("POST_PAIRED_FLUSH_STRAIGHT_CAP");
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a == RAISE_HALF_POT || a == ALL_IN || (a >= RAISE_MIN && to_call > 0) {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if to_call == 0 && !is_three_flush && !board.map(is_four_flush_board).unwrap_or(false) {
                if let Some(r_t) = legal_actions.iter().position(|&a| a == RAISE_THIRD_POT) {
                    probs[r_t] = (probs[r_t] + shifted * 0.65).max(0.55);
                }
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] += shifted * 0.35;
                }
            } else if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] = (probs[c_i] + shifted).max(0.65);
            }
        } else if round == 4
            && (28..=72).contains(&bucket)
            && self.is_guard_enabled("POST_RIVER_OVERBET_CAP")
        {
            let is_three_straight = board
                .map(|b| {
                    let mut ranks = [false; 13];
                    for c in b {
                        ranks[c.rank as usize] = true;
                    }
                    (0..=9).any(|s| (s..s + 4).filter(|&r| ranks[r]).count() >= 3)
                })
                .unwrap_or(false);
            let is_dangerous = board.map(is_dangerous_board).unwrap_or(false)
                || is_three_flush
                || is_three_straight;
            /* Only cap river overbets/shoves on genuinely dangerous boards (3-flush/3-straight/paired),
             * letting GTO/subgame value-bet and value-raise freely on clean rivers! */
            if is_dangerous {
                if to_call > 0 && (bucket < 61 || to_call >= bb * 4) {
                    let mut shifted_raise = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a >= RAISE_MIN {
                            shifted_raise += probs[i];
                            probs[i] = 0.0;
                        }
                    }
                    if shifted_raise > 0.0 {
                        if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                            probs[c_i] += shifted_raise;
                        }
                    }
                } else if to_call == 0 && bucket < 67 {
                    let mut shifted = 0.0f64;
                    for (i, &a) in legal_actions.iter().enumerate() {
                        if a == RAISE_HALF_POT || a == ALL_IN {
                            shifted += probs[i];
                            probs[i] = 0.0;
                        }
                    }
                    if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                        probs[c_i] = (probs[c_i] + shifted).max(0.60);
                    }
                }
            }
        } else if round >= 3
            && (79..=81).contains(&bucket)
            && (board.map(is_four_flush_board).unwrap_or(false)
                || (is_three_flush
                    && board
                        .map(|b| detect_board_texture(b).is_paired)
                        .unwrap_or(false)))
            && self.is_guard_enabled("POST_PAIRED_FLUSH_STRAIGHT_CAP")
        {
            reasons.push("POST_PAIRED_FLUSH_STRAIGHT_CAP");
            let mut shifted = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a == RAISE_HALF_POT || a == ALL_IN || (a >= RAISE_MIN && to_call > 0) {
                    shifted += probs[i];
                    probs[i] = 0.0;
                }
            }
            if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                probs[c_i] += shifted * 0.70;
            }
        }

        if is_three_flush
            && to_call > 0
            && pot_odds >= 0.16
            && bucket < 79
            && bucket != 72
            && self.is_guard_enabled("THREE_FLUSH_NO_RAISE")
        {
            let mut shifted_raise = 0.0f64;
            for (i, &a) in legal_actions.iter().enumerate() {
                if a >= RAISE_MIN {
                    shifted_raise += probs[i];
                    probs[i] = 0.0;
                }
            }
            if shifted_raise > 0.0 {
                reasons.push("THREE_FLUSH_NO_RAISE");
                if let Some(c_i) = legal_actions.iter().position(|&a| a == CALL_CHECK) {
                    probs[c_i] += shifted_raise * 0.70;
                }
            }
        }
    }
}
