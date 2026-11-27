use super::{board, card, combo_weight, hole, remaining_deck_excluding};
use lil_poker_mccfr::cfr::subgame::{
    NUM_HOLE_COMBOS, OpponentReachDistribution, ReachRequest, combo_index,
};
use lil_poker_mccfr::game::holdem::{
    ALL_52_CARDS, CALL_CHECK, Card, NUM_ROUNDS, RAISE_HALF_POT, RAISE_MIN, Rank, RoundHistory,
};
use rand::SeedableRng;
use std::collections::HashMap;

fn used_rem(h: [Card; 2], b: &[Card]) -> Vec<Card> {
    let mut used = Vec::with_capacity(2 + b.len());
    used.extend_from_slice(&h);
    used.extend_from_slice(b);
    remaining_deck_excluding(&used)
}

fn reach_req<'a>(
    remaining_deck: &'a [Card],
    board: &'a [Card],
    round: u8,
    target_player: Option<usize>,
    active_player: Option<usize>,
) -> ReachRequest<'a> {
    ReachRequest {
        remaining_deck,
        board,
        round,
        target_player,
        active_player,
        ..Default::default()
    }
}

#[test]
fn test_opponent_reach_distribution() {
    let (h, b) = (hole("As Ks"), board("Qh Jh Tc"));
    let remaining = used_rem(h, &b);
    let req = reach_req(&remaining, &b, 2, None, None);

    let history_raised = [vec![1u8, 2u8], vec![], vec![], vec![]];
    let dist_raised = OpponentReachDistribution::build(&req, &history_raised);
    assert!(dist_raised.effective_combos() > 100);
    assert!(dist_raised.total_weight > 0.0);
    assert!((dist_raised.cdf.last().unwrap() - 1.0).abs() < 1e-6);

    let mut rng = rand::rngs::SmallRng::seed_from_u64(12345);
    let sample = dist_raised.sample(&mut rng);
    assert_ne!(sample[0], sample[1]);
    assert!(remaining.contains(&sample[0]) && remaining.contains(&sample[1]));

    let history_limped = [vec![1u8, 1u8], vec![], vec![], vec![]];
    let dist_limped = OpponentReachDistribution::build(&req, &history_limped);
    assert_eq!(
        dist_limped.effective_combos(),
        dist_raised.effective_combos()
    );

    let top = dist_raised.top_combos(5);
    assert_eq!(top.len(), 5);
    for i in 0..4 {
        assert!(
            top[i].1 >= top[i + 1].1,
            "Top combos must be sorted descending"
        );
    }
}

#[test]
fn test_opponent_reach_with_strategy_map() {
    let (h, b) = (hole("2s 3h"), board("7d 8c 9h"));
    let remaining = used_rem(h, &b);
    let mut strat_map = HashMap::new();
    strat_map.insert("P:AA/".to_string(), vec![0.0, 0.0, 0.5, 0.3, 0.2, 0.0]);
    strat_map.insert("P:72o/".to_string(), vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);

    let history_raised = [vec![1u8, 2u8], vec![], vec![], vec![]];
    let dist = OpponentReachDistribution::build_with_strategy(
        &reach_req(&remaining, &b, 2, None, None),
        &history_raised,
        Some(&strat_map),
    );
    assert!(dist.total_weight > 0.0);
    assert_eq!(
        dist.combos.len(),
        remaining.len() * (remaining.len() - 1) / 2
    );
}

#[test]
fn test_opponent_reach_distribution_position_aware() {
    let (h, b) = (hole("Kc Qd"), board("Jh Ts 2c"));
    let remaining = used_rem(h, &b);
    let history = [vec![2u8, 1u8], vec![], vec![], vec![]];

    let dist_bot_raised =
        OpponentReachDistribution::build(&reach_req(&remaining, &b, 2, Some(1), Some(0)), &history);
    let dist_opp_raised =
        OpponentReachDistribution::build(&reach_req(&remaining, &b, 2, Some(0), Some(1)), &history);

    let (aa_s, aa_h) = (card("As"), card("Ah"));
    let w_aa_caller = combo_weight(&dist_bot_raised, aa_s, aa_h);
    let w_aa_raiser = combo_weight(&dist_opp_raised, aa_s, aa_h);
    assert!(
        w_aa_raiser > w_aa_caller,
        "AA weight for raiser ({w_aa_raiser}) must exceed AA weight for caller ({w_aa_caller})"
    );
    assert_eq!(w_aa_raiser, 1.0);
    assert_eq!(w_aa_caller, 0.35);

    let w_ako = combo_weight(&dist_opp_raised, card("As"), card("Kh"));
    let w_72 = combo_weight(&dist_opp_raised, card("7s"), card("2h"));
    assert_eq!(
        w_ako, 1.0,
        "AKo for preflop raiser must have full 1.0 weight, got {w_ako}"
    );
    assert_eq!(
        w_72, 0.04,
        "72o for preflop raiser must have trash 0.04 weight, got {w_72}"
    );
}

#[test]
fn test_opponent_reach_distribution_3bet_and_limp_lines() {
    let (h, b) = (hole("2c 3c"), board("7d 8h 9s"));
    let remaining = used_rem(h, &b);
    let req = reach_req(&remaining, &b, 2, Some(1), Some(0));

    let dist_3bet =
        OpponentReachDistribution::build(&req, &[vec![2u8, 4u8], vec![], vec![], vec![]]);
    let (aa_s, aa_h, junk_7, junk_2) = (card("As"), card("Ah"), card("7c"), card("2d"));
    assert_eq!(
        combo_weight(&dist_3bet, aa_s, aa_h),
        1.0,
        "3-bet range must include AA at 1.0 weight"
    );
    assert_eq!(
        combo_weight(&dist_3bet, junk_7, junk_2),
        0.01,
        "3-bet range must filter out 72o junk to 0.01"
    );

    let dist_limp =
        OpponentReachDistribution::build(&req, &[vec![1u8, 1u8], vec![], vec![], vec![]]);
    assert_eq!(
        combo_weight(&dist_limp, aa_s, aa_h),
        0.08,
        "Limped pot caps AA to slowplay frequency 0.08"
    );
    assert_eq!(
        combo_weight(&dist_limp, card("8s"), card("8c")),
        1.00,
        "Limped pot contains mid pairs at 1.00 weight"
    );
}

#[test]
fn test_opponent_reach_distribution_monotone_texture() {
    let (h, b) = (hole("2c 3c"), board("As 7s 4s"));
    let remaining = used_rem(h, &b);
    let history = [vec![2u8, 1u8], vec![2u8], vec![], vec![]];
    let dist =
        OpponentReachDistribution::build(&reach_req(&remaining, &b, 2, Some(0), Some(1)), &history);

    let w_flush = combo_weight(&dist, card("Ks"), card("Qs"));
    let w_no_spade = combo_weight(&dist, card("Kh"), card("Qd"));
    assert!(
        w_flush > w_no_spade * 1.5,
        "Opponent raising on monotone board should weight flush combo ({w_flush}) much higher than no-spade combo ({w_no_spade})"
    );
}

#[test]
fn test_vpip_adapted_bayesian_reach_narrowing() {
    let (b, h) = (board("As Kh 2c"), hole("Qd Jd"));
    let remaining_deck = used_rem(h, &b);
    let history = [vec![RAISE_HALF_POT, CALL_CHECK], vec![], vec![], vec![]];

    let build_vpip = |vpip: f64, pfr: f64| {
        OpponentReachDistribution::build(
            &ReachRequest {
                opp_vpip: Some(vpip),
                opp_pfr: Some(pfr),
                ..reach_req(&remaining_deck, &b, 2, Some(1), Some(0))
            },
            &history,
        )
    };
    let (dist_nit, dist_loose) = (build_vpip(0.15, 0.12), build_vpip(0.75, 0.10));

    let (c7h, c2d) = (card("7h"), card("2d"));
    let p_nit = combo_weight(&dist_nit, c7h, c2d) / dist_nit.total_weight.max(1e-12);
    let p_loose = combo_weight(&dist_loose, c7h, c2d) / dist_loose.total_weight.max(1e-12);
    assert!(
        p_loose > p_nit,
        "Loose VPIP opponent must assign higher relative reach probability to weak offsuit combos ({p_loose:.6} vs {p_nit:.6})"
    );
}

#[test]
fn test_subgame_reach_active_street_actor_separation() {
    let b = board("As Kd Qc");
    let rem = used_rem(hole("Jh Tc"), &b);
    let mut history = [RoundHistory::new(); NUM_ROUNDS];
    history[0].push(CALL_CHECK);
    history[0].push(CALL_CHECK);
    history[1].push(CALL_CHECK);
    history[1].push(RAISE_HALF_POT);

    let no_map: Option<&HashMap<String, Vec<f64>>> = None;
    let target0_as_checker = OpponentReachDistribution::build_with_strategy(
        &reach_req(&rem, &b, 2, Some(0), Some(0)),
        &history,
        no_map,
    );
    let target0_as_bettor = OpponentReachDistribution::build_with_strategy(
        &reach_req(&rem, &b, 2, Some(0), Some(1)),
        &history,
        no_map,
    );

    let air_weight = |dist: &OpponentReachDistribution| -> f64 {
        dist.combos
            .iter()
            .zip(dist.weights.iter())
            .find(|(c, _)| {
                (c[0].rank == Rank::Eight && c[1].rank == Rank::Three)
                    || (c[0].rank == Rank::Three && c[1].rank == Rank::Eight)
            })
            .map_or(0.0, |(_, &w)| w)
    };
    let (w_check, w_bet) = (
        air_weight(&target0_as_checker),
        air_weight(&target0_as_bettor),
    );
    assert!(
        w_check > w_bet * 2.0 && target0_as_checker.total_weight > target0_as_bettor.total_weight,
        "Active-street parity must assign idx 0 (check, air w={w_check:.4}) to target=0 when active=0 rather than idx 1 (bet, air w={w_bet:.4})"
    );
}

#[test]
fn test_subgame_reach_aggression_bluff_multiplier() {
    let b = board("As Kd 7c 2h 3s");
    let rem = used_rem(hole("Qh Qd"), &b);
    let mut history = [RoundHistory::new(); NUM_ROUNDS];
    history[0].push(2);
    history[0].push(CALL_CHECK);
    history[3].push(RAISE_HALF_POT);

    let build_with_agg = |opp_aggression: Option<f64>| {
        OpponentReachDistribution::build(
            &ReachRequest {
                opp_aggression,
                ..reach_req(&rem, &b, 4, Some(0), Some(1))
            },
            &history,
        )
    };
    let (air_c1, air_c2) = (card("5c"), card("9d"));
    let w_aggro = combo_weight(&build_with_agg(Some(3.5)), air_c1, air_c2);
    let w_neutral = combo_weight(&build_with_agg(None), air_c1, air_c2);
    let w_nit = combo_weight(&build_with_agg(Some(0.45)), air_c1, air_c2);
    assert!(
        w_aggro > w_neutral * 2.5 && w_neutral > w_nit * 1.8,
        "River bluff air likelihood must scale with opp_aggression: aggro={w_aggro:.5}, neutral={w_neutral:.5}, nit={w_nit:.5}"
    );
}

#[test]
fn test_combo_index_bijection_and_cross_street_carryover() {
    let mut seen = [false; NUM_HOLE_COMBOS];
    for (i, &c_i) in ALL_52_CARDS.iter().enumerate() {
        for &c_j in &ALL_52_CARDS[(i + 1)..] {
            let idx_ab = combo_index(c_i, c_j);
            assert_eq!(idx_ab, combo_index(c_j, c_i));
            assert!(!seen[idx_ab], "Duplicate combo_index {idx_ab}");
            seen[idx_ab] = true;
        }
    }
    assert!(
        seen.iter().all(|&s| s),
        "Every index 0..1326 must be covered"
    );

    let (h, flop, turn) = (hole("Ac Td"), board("Kh 7s 2c"), board("Kh 7s 2c Qd"));
    let (rem_flop, rem_turn) = (used_rem(h, &flop), used_rem(h, &turn));
    let hist = [
        vec![RAISE_MIN, CALL_CHECK],
        vec![RAISE_HALF_POT, CALL_CHECK],
        vec![RAISE_HALF_POT],
        vec![],
    ];

    let flop_dist =
        OpponentReachDistribution::build(&reach_req(&rem_flop, &flop, 2, Some(0), Some(1)), &hist);
    let cached_flop = flop_dist.to_cached_reach(2, 0, &flop);
    let no_map: Option<&HashMap<String, Vec<f64>>> = None;
    let turn_req = reach_req(&rem_turn, &turn, 3, Some(0), Some(1));
    let turn_dist =
        OpponentReachDistribution::build_with_prior(&turn_req, &hist, no_map, Some(&cached_flop));
    assert!(turn_dist.total_weight > 0.0);
    assert_eq!(
        turn_dist.effective_combos(),
        rem_turn.len() * (rem_turn.len() - 1) / 2
    );

    let cached_flop_tagged = flop_dist.to_cached_reach_at_action(2, 0, &flop, 0);
    let fresh_turn_dist = OpponentReachDistribution::build(&turn_req, &hist);
    let invalidated_turn_dist = OpponentReachDistribution::build_with_prior(
        &turn_req,
        &hist,
        no_map,
        Some(&cached_flop_tagged),
    );
    assert!(
        (invalidated_turn_dist.total_weight - fresh_turn_dist.total_weight).abs() < 1e-9,
        "Stale action-tagged flop cache must invalidate and match fresh full build"
    );
}
