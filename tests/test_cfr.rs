use lil_poker_mccfr::cfr::node::InfosetNode;

#[test]
fn test_cfr_plus_non_negative_regret_clamping() {
    let node = InfosetNode::new(4);

    /* Add negative regrets */
    node.update_regrets_cfr_plus(&[-10.0, -50.0, -100.0, -5.0]);

    /* Regrets must be floored at 0, so strategy must be uniform 0.25 */
    let strat = node.get_strategy();
    assert_eq!(strat, vec![0.25, 0.25, 0.25, 0.25]);

    /* Now add positive regret for action 1 (+20.0). Because floor was 0 (not -50),
    action 1 must immediately have positive regret! */
    node.update_regrets_cfr_plus(&[0.0, 20.0, 0.0, 0.0]);
    let strat2 = node.get_strategy();
    assert_eq!(
        strat2[1], 1.0,
        "Action 1 should have 100% prob immediately without negative debt!"
    );
}

#[test]
fn test_dcfr_regret_discounting() {
    let node = InfosetNode::new(3);

    /* Initial regrets: action 0 has +10.0, action 1 has -10.0, action 2 has 0.0 */
    node.update_regrets_dcfr(&[10.0, -10.0, 0.0], 1.0, 1.0);
    assert!((node.get_regret(0) - 10.0).abs() < 1e-4);
    assert!((node.get_regret(1) - (-10.0)).abs() < 1e-4);

    /* Apply DCFR step with pos_discount = 0.8, neg_discount = 0.5, delta = [0, 0, 0] */
    node.update_regrets_dcfr(&[0.0, 0.0, 0.0], 0.80, 0.50);
    /* Action 0 was +10.0 -> becomes 10.0 * 0.8 = 8.0 */
    assert!(
        (node.get_regret(0) - 8.0).abs() < 1e-4,
        "Positive regret discounted by 0.8"
    );
    /* Action 1 was -10.0 -> becomes -10.0 * 0.5 = -5.0 */
    assert!(
        (node.get_regret(1) - (-5.0)).abs() < 1e-4,
        "Negative regret discounted by 0.5"
    );
}

#[test]
fn test_train_state_incremental_hash_key_and_payoff_parity() {
    use lil_poker_mccfr::cfr::abstraction::{
        append_texture_to_key, compact_board_texture_char, get_holdem_infoset_key_rich_with_bucket,
        get_holdem_infoset_key_with_bucket, hash_from_infoset_key_str,
        hash_holdem_infoset_key_fast, hash_holdem_infoset_key_rich_fast, mix_stack_prefix_hash,
        mix_texture_hash, postflop_equity_bucket, preflop_bucket, stack_depth_prefix,
    };
    use lil_poker_mccfr::cfr::holdem_mccfr::{
        HOLDEM_NUM_ACTIONS, HoldemMCCFRSolver, StepResult, TrainState,
    };
    use lil_poker_mccfr::game::config::{BIG_BLIND, ROUND_FLOP, ROUND_PREFLOP};
    use lil_poker_mccfr::game::holdem::TexasHoldemGame;
    use rand::rngs::SmallRng;
    use rand::{RngExt, SeedableRng};

    let mut rng = SmallRng::seed_from_u64(20_261_124);

    for ep in 0..1500usize {
        let stack_chips = match ep % 3 {
            0 => BIG_BLIND * 15,
            1 => BIG_BLIND * 30,
            _ => BIG_BLIND * 100,
        };
        let rich_history = (ep & 1) == 0;
        let use_texture = (ep % 4) < 2;

        let mut game = TexasHoldemGame::new_random_with_stack(&mut rng, stack_chips);
        let runout = *game.deck_remaining.peek5();
        let (pf0, pf_name0) = preflop_bucket(game.hole[0][0], game.hole[0][1]);
        let (pf1, pf_name1) = preflop_bucket(game.hole[1][0], game.hole[1][1]);
        let pf_idx = [pf0 as u8, pf1 as u8];
        let pf_names = [pf_name0, pf_name1];
        let mut ts = TrainState::new_hand(game.stack_size);

        while !game.is_terminal() {
            let mut g_acts = [0u8; HOLDEM_NUM_ACTIONS];
            let mut t_acts = [0u8; HOLDEM_NUM_ACTIONS];
            let gn = game.legal_actions_buf(&mut g_acts);
            let tn = ts.legal_actions_buf(game.stack_size, &mut t_acts);
            assert_eq!(gn, tn, "legal action count mismatch at ep {ep}");
            assert_eq!(&g_acts[..gn], &t_acts[..tn], "legal actions mismatch");
            assert_eq!(game.round, ts.round);
            assert_eq!(game.current_player(), ts.current_player as usize);
            assert_eq!(game.contributions, ts.contributions);

            let cp = game.current_player();
            let bucket = if game.round == ROUND_PREFLOP {
                pf_idx[cp]
            } else {
                postflop_equity_bucket(&game.hole[cp], &runout[..(game.round as usize + 1)]) as u8
            };
            let tex_char = if use_texture && game.round == ROUND_FLOP {
                Some(compact_board_texture_char(&runout[..3]))
            } else {
                None
            };
            let stack_prefix = stack_depth_prefix(game.stack_size / BIG_BLIND);

            let mut expected_hash = if rich_history {
                hash_holdem_infoset_key_rich_fast(bucket, game.round, &game.history)
            } else {
                hash_holdem_infoset_key_fast(bucket, game.round, &game.history)
            };
            if let Some(tc) = tex_char {
                expected_hash = mix_texture_hash(expected_hash, tc);
            }
            expected_hash = mix_stack_prefix_hash(expected_hash, stack_prefix);

            let fast_hash = ts.infoset_hash(bucket, tex_char, game.stack_size);
            assert_eq!(fast_hash, expected_hash, "incremental hash mismatch");

            let mut expected_key = if rich_history {
                get_holdem_infoset_key_rich_with_bucket(
                    &game.hole[cp],
                    bucket,
                    game.round,
                    &game.history,
                )
            } else {
                get_holdem_infoset_key_with_bucket(
                    &game.hole[cp],
                    bucket,
                    game.round,
                    &game.history,
                )
            };
            if let Some(tc) = tex_char {
                expected_key = append_texture_to_key(&expected_key, tc);
            }
            if !stack_prefix.is_empty() {
                expected_key = format!("{stack_prefix}{expected_key}");
            }

            let fast_key = ts.format_infoset_key(
                pf_names[cp],
                bucket,
                tex_char,
                game.stack_size,
                rich_history,
            );
            assert_eq!(fast_key, expected_key, "formatted infoset key mismatch");
            assert_eq!(
                hash_from_infoset_key_str(&fast_key),
                fast_hash,
                "key-to-hash roundtrip mismatch for {fast_key}"
            );

            let pick = rng.random_range(0..gn);
            let act = g_acts[pick];
            game = game.apply_action(act);
            match ts.step(act, game.stack_size, game.is_wet_flop, rich_history) {
                StepResult::Continue(next_ts) => {
                    assert!(!game.is_terminal(), "expected non-terminal game");
                    ts = next_ts;
                }
                StepResult::FoldWin {
                    loser_contrib,
                    loser,
                } => {
                    assert!(game.is_terminal(), "expected terminal fold");
                    let rets = game.get_returns();
                    assert!((rets[loser] - (-(loser_contrib as f64))).abs() < 1e-9);
                    assert!((rets[1 - loser] - (loser_contrib as f64)).abs() < 1e-9);
                    break;
                }
                StepResult::Showdown { contributions } => {
                    assert!(game.is_terminal(), "expected terminal showdown");
                    assert_eq!(contributions, game.contributions);
                    break;
                }
            }
        }
    }

    /* Verify FlatInfosetTable raw checkpoint lossless round-trip */
    let solver = HoldemMCCFRSolver::with_options(true, true, true, 65_536, 64)
        .with_multi_stack(true)
        .with_board_texture(true);
    solver.train(2_000, 2, 0);
    let ckpt_path = "/tmp/test_flat_table_roundtrip.ckpt";
    let saved_n = solver
        .save_raw_checkpoint(ckpt_path)
        .expect("save raw checkpoint");
    assert!(saved_n > 0);
    let before = solver.export_strategy();

    let solver2 = HoldemMCCFRSolver::with_options(true, true, true, 65_536, 64)
        .with_multi_stack(true)
        .with_board_texture(true);
    let loaded_n = solver2
        .warm_start_from_file(ckpt_path)
        .expect("load raw checkpoint");
    assert_eq!(loaded_n, saved_n);
    let after = solver2.export_strategy();
    assert_eq!(before.len(), after.len());
    for (k, v1) in &before {
        let v2 = after.get(k).expect("missing key after ckpt restore");
        for (a, b) in v1.iter().zip(v2.iter()) {
            assert!((a - b).abs() < 1e-9, "strategy mismatch on {k}");
        }
    }
    let _ = std::fs::remove_file(ckpt_path);
}
