/* Precomputed Tactical Guard Input & Context */
use super::super::tracker::OpponentTracker;
use super::helpers::{is_three_straight_board, river_nut_blocker_allowed};
use crate::cfr::abstraction::{
    detect_board_texture, detect_river_nut_blocker, is_dangerous_board, is_four_flush_board,
    is_four_straight_board, preflop_strength_tier,
};
use crate::game::config::{ALL_IN, RAISE_MIN};
use crate::game::holdem::Card;

pub(in crate::cfr::opponent_model) struct GuardInput<'a> {
    pub legal_actions: &'a [u8],
    pub bucket: usize,
    pub round: u8,
    pub to_call: i32,
    pub pot: i32,
    pub hole: Option<&'a [Card; 2]>,
    pub board: Option<&'a [Card]>,
    pub active_opponents: usize,
    pub bb: i32,
    pub my_chips: i32,
    pub eff_allin_threshold: i32,
    pub pot_odds: f64,
    pub is_three_flush: bool,
    pub from_subgame: bool,
}

pub(in crate::cfr::opponent_model) struct GuardContext<'a> {
    pub legal_actions: &'a [u8],
    pub bucket: usize,
    pub round: u8,
    pub to_call: i32,
    pub pot: i32,
    pub hole: Option<&'a [Card; 2]>,
    pub board: Option<&'a [Card]>,
    pub active_opponents: usize,
    pub bb: i32,
    pub my_chips: i32,
    pub eff_allin_threshold: i32,
    pub pot_odds: f64,
    pub is_three_flush: bool,
    pub from_subgame: bool,
    pub is_preflop: bool,
    pub is_allin_call: bool,
    pub eff_stack_bb: i32,
    pub is_short_stack: bool,
    pub can_raise: bool,
    pub is_passive_station: bool,
    pub is_overbet_bluffer: bool,
    pub is_known_maniac: bool,
    pub is_extreme_shove_bot: bool,
    pub is_hyper_aggressor: bool,
    pub is_tight_passive_bb: bool,
    pub is_tight_passive_postflop: bool,
    pub is_paired_board: bool,
    pub is_four_flush_board: bool,
    pub is_four_straight_board: bool,
    pub is_four_board_danger: bool,
    pub is_dangerous_board: bool,
    pub is_three_straight_board: bool,
    pub never_fold_postflop: bool,
    pub allow_pot_commit_rescue: bool,
    pub has_river_nut_blocker: bool,
}

impl<'a> GuardContext<'a> {
    pub(super) fn new(tracker: &OpponentTracker, input: GuardInput<'a>) -> Self {
        let is_preflop = input.round == 1;
        let is_allin_call = input.my_chips > 0 && input.to_call >= input.my_chips;
        let eff_stack_bb = if input.my_chips > 0 {
            input.my_chips / input.bb.max(1)
        } else {
            100
        };
        let is_short_stack = input.my_chips > 0 && eff_stack_bb <= 25;
        let can_raise = !is_allin_call
            && input
                .legal_actions
                .iter()
                .any(|&a| (RAISE_MIN..=ALL_IN).contains(&a));

        let sm_raise = tracker.smoothed_raise_ratio();
        let sm_pfr = tracker.smoothed_pfr();
        let sm_vpip = tracker.smoothed_vpip();

        let is_passive_station = tracker.is_passive_station();
        let is_overbet_bluffer = tracker.is_overbet_bluffer();
        let is_known_maniac = tracker.total_actions() >= 10
            && (sm_raise > 0.36
                || (sm_pfr > 0.48 && sm_raise > 0.28)
                || (sm_vpip > 0.80 && sm_raise > 0.28));
        let is_extreme_shove_bot = tracker.total_hands >= 8 && sm_raise > 0.42 && sm_vpip > 0.68;
        let is_hyper_aggressor = tracker.total_hands >= 12
            && (tracker.raise_ratio() > 0.45
                || (tracker.pfr_ratio() > 0.58 && tracker.raise_ratio() > 0.35));
        let is_tight_passive_bb = tracker.total_hands >= 8
            && ((sm_vpip < 0.30 && sm_pfr < 0.18)
                || (tracker.vpip_ratio() < 0.28 && tracker.pfr_ratio() < 0.18));
        let is_tight_passive_postflop = tracker.total_hands >= 8
            && ((sm_vpip < 0.30 && sm_pfr < 0.18)
                || (tracker.vpip_ratio() < 0.26 && tracker.pfr_ratio() < 0.16));

        let is_paired_board = input
            .board
            .map(|b| detect_board_texture(b).is_paired)
            .unwrap_or(false);
        let is_four_flush = input.board.map(is_four_flush_board).unwrap_or(false);
        let is_four_straight = input.board.map(is_four_straight_board).unwrap_or(false);
        let is_four_board_danger = is_four_straight || is_four_flush;
        let is_dangerous = input.board.map(is_dangerous_board).unwrap_or(false);
        let is_three_straight = input.board.map(is_three_straight_board).unwrap_or(false);

        let never_fold_postflop = !is_preflop
            && (input.bucket >= 83
                || (input.bucket == 82 && !is_four_flush && input.to_call < input.pot * 2)
                || (input.bucket == 81 && !is_four_flush && !is_paired_board));
        let allow_pot_commit_rescue = !(is_preflop
            && preflop_strength_tier(input.bucket) >= 7
            && input.to_call > input.bb * 2);
        let has_river_nut_blocker = input.round == 4
            && input.active_opponents == 1
            && input.to_call == 0
            && input.bucket < 28
            && !is_passive_station
            && (tracker.total_actions() < 6 || tracker.fold_ratio() >= 0.30)
            && river_nut_blocker_allowed(input.pot, input.bb, input.my_chips)
            && input
                .hole
                .zip(input.board)
                .map(|(h, b)| detect_river_nut_blocker(h, b))
                .unwrap_or(false);

        Self {
            legal_actions: input.legal_actions,
            bucket: input.bucket,
            round: input.round,
            to_call: input.to_call,
            pot: input.pot,
            hole: input.hole,
            board: input.board,
            active_opponents: input.active_opponents,
            bb: input.bb,
            my_chips: input.my_chips,
            eff_allin_threshold: input.eff_allin_threshold,
            pot_odds: input.pot_odds,
            is_three_flush: input.is_three_flush,
            from_subgame: input.from_subgame,
            is_preflop,
            is_allin_call,
            eff_stack_bb,
            is_short_stack,
            can_raise,
            is_passive_station,
            is_overbet_bluffer,
            is_known_maniac,
            is_extreme_shove_bot,
            is_hyper_aggressor,
            is_tight_passive_bb,
            is_tight_passive_postflop,
            is_paired_board,
            is_four_flush_board: is_four_flush,
            is_four_straight_board: is_four_straight,
            is_four_board_danger,
            is_dangerous_board: is_dangerous,
            is_three_straight_board: is_three_straight,
            never_fold_postflop,
            allow_pot_commit_rescue,
            has_river_nut_blocker,
        }
    }
}
