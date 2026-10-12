/* Leduc Poker Exploitability (Best Response / NashConv) and Model Evaluation */
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use super::runner::{LeducStrategy, Strategy};
use crate::game::card::ALL_CARDS;
use crate::game::leduc::LeducGame;

/* Simulation results against random agent */
#[derive(Debug, Clone, Copy)]
pub struct SimResult {
    pub win_pct: f64,
    pub avg_chips: f64,
    pub mbb: f64,
    pub mbb_se: f64,
}

/* Tabular Strategy Display */
pub fn display_sample_strategy(strategy: &Strategy, verbose: bool) {
    println!("\n--- Strategy Profile Highlights ---");
    println!(
        "{:<14} | {:<8} | {:<8} | {:<8}",
        "Infoset Key", "Fold", "Call/Check", "Raise"
    );
    println!("--------------------------------------------------");

    let sample_keys = if verbose {
        /* Show 20 representative keys */
        vec![
            "Jc/_//",
            "Jc/_/r/",
            "Qc/_//",
            "Qc/_/r/",
            "Kc/_//",
            "Kc/_/r/",
            "Kc/_/c/",
            "Kc/_/cr/",
            "Jc/Jd/cc/",
            "Jc/Jd/cc/r",
            "Jc/Qd/cc/",
            "Jc/Qd/cc/r",
            "Kc/Jd/cc/",
            "Kc/Jd/cc/r",
            "Kc/Kd/cc/",
            "Kc/Kd/cc/r",
            "Qc/Qd/cc/",
            "Qc/Qd/cc/r",
            "Jc/_/crr/",
            "Kc/_/crr/",
        ]
    } else {
        /* Show core 8 preflop & postflop keys */
        vec![
            "Jc/_//",
            "Jc/_/r/",
            "Qc/_//",
            "Qc/_/r/",
            "Kc/_//",
            "Kc/_/r/",
            "Jc/Jd/cc/",
            "Kc/Kd/cc/",
        ]
    };

    for &key in &sample_keys {
        if let Some(strat) = strategy.get(key) {
            println!(
                "{:<14} | {:<8.3} | {:<8.3} | {:<8.3}",
                key, strat[0], strat[1], strat[2]
            );
        }
    }
}

/* Simulation against Random Agent */
pub fn simulate_vs_random(
    strategy: &Strategy,
    hands: u64,
    model_player: usize,
    seed: u64,
) -> SimResult {
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut total_payoff = 0.0f64;
    let mut sq_payoff = 0.0f64;
    let mut wins = 0u64;

    for _ in 0..hands {
        let mut game = LeducGame::new_random(&mut rng);
        loop {
            if game.is_terminal() {
                break;
            }
            let cp = game.current_player();
            let act = if cp == model_player {
                sample_model_action(strategy, &game, cp, &mut rng)
            } else {
                let legal = game.legal_actions();
                legal[rng.gen_range(0..legal.len())]
            };
            game = game.apply_action(act);
        }

        let payoff = game.get_returns()[model_player];
        if payoff > 0.0 {
            wins += 1;
        }
        total_payoff += payoff;
        sq_payoff += payoff * payoff;
    }

    let avg_chips = total_payoff / hands as f64;
    let var = sq_payoff / hands as f64 - avg_chips * avg_chips;
    let se_chips = (var / hands as f64).sqrt();

    /* Ante = 1 chip = 1 big blind in Leduc unit */
    let mbb = avg_chips * 1000.0;
    let mbb_se = se_chips * 1000.0;
    let win_pct = (wins as f64 / hands as f64) * 100.0;

    SimResult {
        win_pct,
        avg_chips,
        mbb,
        mbb_se,
    }
}

pub fn sample_model_action(
    strategy: &Strategy,
    game: &LeducGame,
    player: usize,
    rng: &mut SmallRng,
) -> u8 {
    let legal = game.legal_actions();
    let key = game.infoset_key(player);
    let n = legal.len();

    let probs: Vec<f64> = if let Some(s) = strategy.get(&key) {
        let raw: Vec<f64> = legal.iter().map(|&a| s[a as usize].max(0.0)).collect();
        let sum: f64 = raw.iter().sum();
        if sum > 1e-12 {
            raw.iter().map(|p| p / sum).collect()
        } else {
            vec![1.0 / n as f64; n]
        }
    } else {
        vec![1.0 / n as f64; n]
    };

    let r: f64 = rng.gen();
    let mut cum = 0.0;
    for (i, &p) in probs.iter().enumerate() {
        cum += p;
        if r < cum {
            return legal[i];
        }
    }
    legal[n - 1]
}

/* Best Response Tree Traversal (NashConv) */
pub fn compute_exploitability(strategy: &LeducStrategy) -> f64 {
    let cards = &ALL_CARDS;
    let n = cards.len();
    let mut br0_sum = 0.0f64;
    let mut br1_sum = 0.0f64;

    for i in 0..n {
        for j in 0..n {
            if j == i {
                continue;
            }
            let rem_boards: Vec<_> = (0..n)
                .filter(|&k| k != i && k != j)
                .map(|k| cards[k])
                .collect();

            let dummy = rem_boards[0];
            let g = LeducGame::new_with_cards(cards[i], cards[j], dummy);

            br0_sum += br_recurse_r1(&g, &rem_boards, 0, strategy);
            br1_sum += br_recurse_r1(&g, &rem_boards, 1, strategy);
        }
    }

    (br0_sum / 30.0) + (br1_sum / 30.0)
}

fn br_recurse_r1(
    game: &LeducGame,
    rem_boards: &[crate::game::card::Card],
    br_player: usize,
    strategy: &LeducStrategy,
) -> f64 {
    if game.is_terminal() {
        return game.get_returns()[br_player];
    }

    if game.round == 2 {
        let sum: f64 = rem_boards
            .iter()
            .map(|&b| {
                let mut g2 = LeducGame::new_with_cards(game.hole[0], game.hole[1], b);
                for &a in &game.history[0] {
                    g2 = g2.apply_action(a);
                }
                br_recurse_r2(&g2, br_player, strategy)
            })
            .sum();
        return sum / rem_boards.len() as f64;
    }

    let cp = game.current_player();
    let actions = game.legal_actions();

    if cp == br_player {
        actions
            .iter()
            .map(|&a| br_recurse_r1(&game.apply_action(a), rem_boards, br_player, strategy))
            .fold(f64::NEG_INFINITY, f64::max)
    } else {
        let probs = get_action_probs_evaluate(strategy, game, cp);
        actions
            .iter()
            .zip(probs.iter())
            .map(|(&a, &p)| {
                p * br_recurse_r1(&game.apply_action(a), rem_boards, br_player, strategy)
            })
            .sum()
    }
}

fn br_recurse_r2(game: &LeducGame, br_player: usize, strategy: &LeducStrategy) -> f64 {
    if game.is_terminal() {
        return game.get_returns()[br_player];
    }

    let cp = game.current_player();
    let actions = game.legal_actions();

    if cp == br_player {
        actions
            .iter()
            .map(|&a| br_recurse_r2(&game.apply_action(a), br_player, strategy))
            .fold(f64::NEG_INFINITY, f64::max)
    } else {
        let probs = get_action_probs_evaluate(strategy, game, cp);
        actions
            .iter()
            .zip(probs.iter())
            .map(|(&a, &p)| p * br_recurse_r2(&game.apply_action(a), br_player, strategy))
            .sum()
    }
}

fn get_action_probs_evaluate(strategy: &LeducStrategy, game: &LeducGame, cp: usize) -> Vec<f64> {
    let legal = game.legal_actions();
    let key = game.infoset_key(cp);
    let n = legal.len();

    if let Some(s) = strategy.get(&key) {
        let raw: Vec<f64> = legal.iter().map(|&a| s[a as usize].max(0.0)).collect();
        let sum: f64 = raw.iter().sum();
        if sum > 1e-12 {
            return raw.iter().map(|p| p / sum).collect();
        }
    }
    vec![1.0 / n as f64; n]
}
