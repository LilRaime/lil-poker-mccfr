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
