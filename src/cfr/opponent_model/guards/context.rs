/*! Precomputed Tactical Guard Input & Context */
use super::super::tracker::OpponentTracker;
use super::helpers::{is_three_straight_board, river_nut_blocker_allowed};
use crate::cfr::abstraction::{
    BUCKET_NUT_FLUSH, BUCKET_SECOND_NUT_FLUSH, BUCKET_STRONG_DRAW_MIN, BUCKET_WEAK_PAIR_MIN,
    PostflopBucket, detect_board_texture, detect_river_nut_blocker, is_dangerous_board,
    is_four_flush_board, is_four_straight_board, preflop_strength_tier,
};
use crate::game::config::{ALL_IN, NUM_SUITS, RAISE_MIN, ROUND_PREFLOP, ROUND_RIVER, ROUND_TURN};
use crate::game::holdem::{Card, Rank};

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
    pub post_bucket: PostflopBucket,
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
    pub is_sticky_caller: bool,
    pub is_overbet_bluffer: bool,
    pub is_known_maniac: bool,
    pub is_extreme_shove_bot: bool,
    pub is_hyper_aggressor: bool,
    pub is_tight_passive_bb: bool,
    pub is_ultra_sticky_bb: bool,
    pub is_tight_passive_postflop: bool,
    pub is_low_pfr_aggressor: bool,
    pub is_frequent_sb_limper: bool,
    pub is_paired_board: bool,
    pub is_four_flush_board: bool,
    pub is_four_straight_board: bool,
    pub is_four_board_danger: bool,
    pub is_dangerous_board: bool,
    pub is_three_straight_board: bool,
    pub never_fold_postflop: bool,
    pub allow_pot_commit_rescue: bool,
    pub has_river_nut_blocker: bool,
    pub has_turn_one_card_fd: bool,
}

impl<'a> GuardContext<'a> {
    pub(super) fn new(tracker: &OpponentTracker, input: GuardInput<'a>) -> Self {
        let is_preflop = input.round == ROUND_PREFLOP;
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

        let profile = tracker.tactical_profile();

        let is_paired_board = input
            .board
            .is_some_and(|b| detect_board_texture(b).is_paired);
        let is_four_flush = input.board.is_some_and(is_four_flush_board);
        let is_four_straight = input.board.is_some_and(is_four_straight_board);
        let is_four_board_danger = is_four_straight || is_four_flush;
        let is_dangerous = input.board.is_some_and(is_dangerous_board);
        let is_three_straight = input.board.is_some_and(is_three_straight_board);

        let post_bucket = if is_preflop {
            PostflopBucket(0)
        } else {
            PostflopBucket::from_usize(input.bucket)
        };
        let never_fold_postflop = !is_preflop
            && (input.bucket >= BUCKET_NUT_FLUSH
                || (input.bucket == BUCKET_SECOND_NUT_FLUSH
                    && !is_four_flush
                    && input.to_call < input.pot * 2)
                || (post_bucket.as_u8() == PostflopBucket::GOOD_TWO_CARD_FLUSH
                    && !is_four_flush
                    && !is_paired_board));
        let allow_pot_commit_rescue = !(is_preflop
            && preflop_strength_tier(input.bucket) >= 7
            && input.to_call > input.bb * 2);
        let has_river_nut_blocker = input.round == ROUND_RIVER
            && input.active_opponents == 1
            && input.to_call == 0
            && input.bucket < BUCKET_WEAK_PAIR_MIN
            && !profile.is_passive_station
            && (tracker.total_actions() < 6 || tracker.fold_ratio() >= 0.30)
            && river_nut_blocker_allowed(input.pot, input.bb, input.my_chips)
            && input
                .hole
                .zip(input.board)
                .is_some_and(|(h, b)| detect_river_nut_blocker(h, b));

        let has_turn_one_card_fd = input.round == ROUND_TURN
            && input.is_three_flush
            && !is_four_flush
            && input.bucket < BUCKET_STRONG_DRAW_MIN
            && input.hole.zip(input.board).is_some_and(|(h, b)| {
                let mut suit_counts = [0u8; NUM_SUITS];
                for c in b {
                    suit_counts[c.suit as usize] += 1;
                }
                suit_counts
                    .iter()
                    .position(|&cnt| cnt == 3)
                    .is_some_and(|fs| {
                        h.iter().any(|c| {
                            c.suit as usize == fs && (c.rank as usize) >= (Rank::Jack as usize)
                        })
                    })
            });

        Self {
            legal_actions: input.legal_actions,
            bucket: input.bucket,
            post_bucket,
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
            is_passive_station: profile.is_passive_station,
            is_sticky_caller: profile.is_sticky_caller,
            is_overbet_bluffer: profile.is_overbet_bluffer,
            is_known_maniac: profile.is_known_maniac,
            is_extreme_shove_bot: profile.is_extreme_shove_bot,
            is_hyper_aggressor: profile.is_hyper_aggressor,
            is_tight_passive_bb: profile.is_tight_passive_bb,
            is_ultra_sticky_bb: profile.is_ultra_sticky_bb,
            is_tight_passive_postflop: profile.is_tight_passive_postflop,
            is_low_pfr_aggressor: profile.is_low_pfr_aggressor,
            is_frequent_sb_limper: profile.is_frequent_sb_limper,
            is_paired_board,
            is_four_flush_board: is_four_flush,
            is_four_straight_board: is_four_straight,
            is_four_board_danger,
            is_dangerous_board: is_dangerous,
            is_three_straight_board: is_three_straight,
            never_fold_postflop,
            allow_pot_commit_rescue,
            has_river_nut_blocker,
            has_turn_one_card_fd,
        }
    }
}
