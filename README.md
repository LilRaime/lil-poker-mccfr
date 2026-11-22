# lil-poker-mccfr 🃏

A high-performance parallel poker solver, online subgame resolver, and exploitative bot client written in Rust (Edition 2024). Implements **External Sampling MCCFR with Discounted CFR (DCFR)**, **CFR+**, and **Real-Time Subgame Search (Pluribus/Libratus-style)** for both 52-card Texas Hold'em and Leduc Hold'em.

Powered by an ultra-fast **$O(1)$ bitwise 7-card hand evaluator** capable of sustaining **> 1,300,000+ iterations/second** on consumer multicore CPUs.

---

## Key Features

- **Blazing Fast Performance ($> 1.3\text{M}$ iter/s)** — Pure $O(1)$ bitwise 7-card evaluator (zero-alloc, zero-sort, bitmask straights/flushes, x86 hardware `LZCNT` kickers) + lock-free Rayon & 4096-shard DashMap parallelism.
- **Discounted CFR (DCFR, Pluribus algorithm)** — Epoch-based regret & strategy discount schedules ($\alpha=1.5$, negative regret discount $\max(\frac{t}{t+1}, 0.75)$, $\gamma=2.0$) that clear early-iteration noise and converge dramatically faster than vanilla CFR+.
- **Real-Time Subgame Search with Explicit Reach Ranges** — Depth-limited Flop (medium/big pots $\ge 8\text{ BB}$), Turn, and River subgame resolver with $O(\log K)$ Bayesian reach range sampling across all candidate combos (`OpponentReachDistribution`), dynamically scaling iteration budget based on pot size, street, and bet pressure (up to 6,000+ iterations).
- **Safe Resolving (Burch et al. 2014)** — Blends real-time subgame policies with robust GTO fallback bounds (85% solved / 15% fallback; 35% / 65% for air and board-pair hands) to prevent gift-giving and opponent out-of-distribution exploitation.
- **Pseudo-Harmonic Action Translation (Ganzfried & Sandholm 2013)** — Probabilistically maps continuous, off-tree opponent bet amounts to nearest discrete actions preserving exact pot odds and expectation value.
- **Texture-Aware & Potential Abstraction** — 169 canonical preflop hand groups + 88 postflop equity buckets (`0..=87`) conditioned on board texture (Monotone, TwoTone, Rainbow, paired/connected boards) and combo/redraw potential (NFD, OESD, backdoor/1-card flush redraws), with optional compact 4-state postflop board-texture keys (`d`/`w`/`p`/`m`) and unified multi-stack blueprints (`15 BB` / `30 BB` / `100 BB`).
- **Regret-Based Pruning (RBP)** — Skips 95% of deeply negative regret branches using a dynamic pot/stack-relative threshold (`-3.5 * pot`, floored at 6% of stack, when action probability `< 0.005`), never pruning `FOLD`, River actions, or all-in calls.
- **Exploitative Opponent Modeling & Priority Guard Pipeline** — Tracks VPIP, PFR, 3-bet frequency, fold-to-steal/cbet, barrel stats, and classifies opponents (`CallingStation`, `Maniac`, `Rock`, `TAG`, `LAG`) with Wilson score intervals, modular guard tiers (`guards/tier1_boosts`, `subgame`, `tier2_preflop`, `tier2_shove`, `tier2_caps`, `multiway`, and Tier-3 barrel/pot-commitment guards in `station.rs`), and short-stack Nash push/call defense (`policy.rs`).
- **Showdown Learning & Sizing Tells** — Detects opponent bluffing, trapping habits, and bet-sizing tells (small vs large bet hand strengths) from revealed showdown hole cards, dynamically tuning bluff-catching and barrel frequencies.
- **Geometric Street-Aware Bet Sizing & Overbets** — Dynamic pot-scaled sizing: 2.5x BB open-raise preflop, 50% pot on dry flop, 75% pot on wet flop/turn, 125% polarized overbet on river, and geometric stack commitment utility.
- **Purified Action Defense** — Clamps Monte Carlo noise and prevents accidental folds of premium hands and strong made flushes when facing heavy bets or all-in shoves.
- **Live Server Bot Client (`play_live`)** — Native async WebSocket & REST client connecting directly to [lil-poker](https://github.com/LilRaime/lil-poker) HU & 6-max rooms with multiway seat tracking, live showdown updates, stack-commit execution guards, and safe non-escalating rejected-action fallbacks (`src/live/client/`).
- **Exact Full-Tree Vanilla CFR+ (Leduc)** — Full game tree traversal solver for Leduc Hold'em with exact Best Response / NashConv computation.

---

## Benchmark & Performance Comparison

Measured on an AMD Ryzen multicore processor (16 parallel threads):

| Metric | Previous Engine | Optimized Engine ($O(1)$ Bitwise + RBP + DCFR) | Speedup |
| :--- | :--- | :--- | :--- |
| **7-Card Evaluation** | Array sort (`sort_by_key`) + allocations | Pure bitmask shifts + `leading_zeros()` | **$\sim 3.8\times$** |
| **Hold'em MCCFR Throughput** | ~20,000 – 50,000 iter/s | **1,100,000 – 1,400,000+ iter/s** | **$\sim 28\times$** |
| **50M Iterations Training** | ~41 minutes | **~35 seconds** | **$\sim 70\times$ faster** |
| **100M Iterations Training** | ~1.5 hours | **~1 minute 15 seconds** | **$\sim 70\times$ faster** |
| **1.6 Billion Iterations** | ~24 hours | **~20 minutes** | **$\sim 70\times$ faster** |
| **Memory Footprint** | ~500 MB | **~180 MB** (zero-heap recursion) | **$2.7\times$ lighter** |

---

## Algorithm Architecture

### 1. Counterfactual Regret Minimization (CFR) & DCFR
CFR iteratively minimizes regret for not having played alternative actions. The average strategy across iterations converges to an $\varepsilon$-Nash equilibrium.
* **DCFR (Brown & Sandholm 2019):** Applies epoch-based polynomial weighting:
  $$\text{Positive regret discount: } \frac{t^\alpha}{t^\alpha + 1} \quad (\alpha = 1.5)$$
  $$\text{Negative regret discount: } \max\!\left(\frac{t}{t+1},\, 0.75\right)$$
  $$\text{Strategy contribution weight: } \left(\frac{t}{t+1}\right)^\gamma \quad (\gamma = 2.0)$$
  This clears early exploration noise across epochs without requiring heuristic restart phases.

### 2. $O(1)$ Bitwise 7-Card Hand Evaluator
Replaces traditional card sorting with compact 16-bit masks:
* `rank_mask: u16` — 13 bits indicating presence of each rank.
* `suit_masks: [u16; 4]` — ranks present within each suit.
* Straights and straight flushes are identified via 3 shift-and-AND operations:
  ```rust
  let st_mask = mask & (mask >> 1) & (mask >> 2) & (mask >> 3) & (mask >> 4);
  ```
* Kickers and top ranks are extracted using the x86 `LZCNT`/`BSR` hardware instruction (`15 - mask.leading_zeros()`).

### 3. Adaptive Subgame Resolving
At Turn, River, and medium-to-large Flop pots ($\ge 8\text{ BB}$ / `800` chips at 100-chip BB), the bot constructs a localized subgame conditioned on Bayesian opponent reach ranges. The iteration budget scales dynamically:
$$\text{Budget} = \text{Base} \times M_{\text{pot}} \times M_{\text{street}} \times M_{\text{pressure}}$$
* Small pots ($\le 6\text{ BB}$ / `600` chips): $0.55\times$ base for instant millisecond responses.
* Medium-to-large pots ($\ge 20\%$ / $\ge 40\%$ / $\ge 80\%$ of starting stack): $1.25\times$ / $1.6\times$ / up to $2.2\times$ base for deep precision.
* Turn nodes: $1.1\times$ boost; River nodes: $1.35\times$ boost (terminal street with zero chance nodes).
* Facing large bet / all-in ($\ge 10\text{ BB}$ / `1,000` chips): $1.25\times$ boost.

---

## Project Structure

```
src/
├── lib.rs
├── main.rs                          # CLI: Leduc Hold'em MCCFR training (`train`)
├── game/
│   ├── mod.rs
│   ├── card.rs                      # 6-card Leduc & deck representations
│   ├── config.rs                    # Centralized game dimensions, action codes, stack/blind floors, and solver constants
│   ├── leduc.rs                     # Leduc Hold'em rules & state transitions
│   └── holdem/                      # 52-card Texas Hold'em game engine
│       ├── mod.rs
│       ├── types.rs                 # Card, Rank, Suit, Board, RoundHistory, DeckRemaining
│       ├── actions.rs               # Geometric bet sizing (`calculate_geometric_bet_size`) & action constant re-exports
│       ├── eval.rs                  # O(1) bitwise 7-card evaluator with LZCNT kickers
│       └── state.rs                 # Game state transitions, contributions, and showdown
├── cfr/
│   ├── mod.rs
│   ├── node.rs                      # Lock-free InfosetNode (AtomicI64, CFR+ & DCFR)
│   ├── holdem_mccfr.rs              # Multi-threaded Hold'em MCCFR solver with DCFR & RBP
│   ├── mccfr.rs                     # Leduc MCCFR solver
│   ├── vanilla.rs                   # Exact full-tree Vanilla CFR+ solver (Leduc)
│   ├── fallback/                    # Robust GTO fallback bounds & lookup index
│   │   ├── mod.rs
│   │   ├── heuristic.rs             # Preflop & postflop GTO-aligned equity heuristics
│   │   └── index.rs                 # StrategyLookupIndex for fast nearest-key fallback blending
│   ├── subgame/                     # Real-Time Subgame Solver (adaptive budgeting & safe resolving)
│   │   ├── mod.rs
│   │   ├── solver.rs                # SubgameSolver & dynamic iteration budget
│   │   ├── traversal.rs             # Multi-threaded CFR+ traversal over depth-limited subtrees
│   │   └── reach/                   # Bayesian opponent reach range distribution
│   │       ├── mod.rs
│   │       ├── preflop.rs           # Preflop range weights conditioned on open/3bet/limp & PFR
│   │       └── postflop.rs          # Street/texture/sizing range filtering
│   ├── abstraction/                 # State & card abstraction pipeline
│   │   ├── mod.rs
│   │   ├── preflop.rs               # 169 canonical preflop hand buckets
│   │   ├── draws.rs                 # Flush/straight combo draw & 1-card redraw detection
│   │   ├── ehs.rs                   # Expected Hand Strength & hand-potential helpers
│   │   ├── texture.rs               # Board texture classification (Rainbow, Two-Tone, Monotone, Paired)
│   │   ├── keys.rs                  # Zero-allocation 64-bit FNV-1a hashing & rich infoset keys
│   │   └── postflop/                # 88 postflop equity buckets (0..=87) & board strength tiers
│   │       ├── mod.rs               # Category boundaries (BUCKET_* constants) & routing
│   │       ├── high_card.rs         # Unpaired high-card & draw bucket mapping (0..=27)
│   │       └── pairs.rs             # One-pair, overpair, two-pair & paired-board bucket mapping (28..=64)
│   └── opponent_model/              # Opponent tracking & dynamic exploitation
│       ├── mod.rs
│       ├── classifier.rs            # Archetypes (CallingStation, Maniac, Rock, TAG, LAG)
│       ├── policy.rs                # Purified action selection, short-stack Nash push/call & effective stack calc
│       ├── exploit.rs               # Dynamic sizing & steal/value exploitation
│       ├── station.rs               # Calling-station value extraction & Tier-3 terminal barrel/pot-commit guards
│       ├── multiway.rs              # 6-max positional open tiers & multiway preflop/postflop discipline
│       ├── tracker/                 # Per-opponent stats & persistence
│       │   ├── mod.rs               # OpponentTracker state, street counters & showdown learning
│       │   ├── metrics.rs           # Smoothed ratios, Wilson intervals, trap/bluff/sizing-tell detectors
│       │   └── profile.rs           # Persistent JSON opponent profile load/save with /tmp fallback
│       └── guards/                  # Priority-ordered tactical guard pipeline
│           ├── mod.rs               # Priority guard pipeline orchestrator
│           ├── context.rs           # Precomputed GuardContext & board danger flags
│           ├── helpers.rs           # Zero-cost probability shift/cap/force helpers
│           ├── subgame.rs           # Turn/River SubgameSolver safety invariants
│           ├── tier1_boosts.rs      # Tier 1: soft value boosts, button steals & river nut-blocker bluffs
│           ├── tier2_preflop.rs     # Tier 2A: preflop open/3-bet/4-bet discipline & pair set-mining
│           ├── tier2_shove.rs       # Tier 2A: preflop/postflop shove detection, rescue & range-equity folds
│           └── tier2_caps.rs        # Tier 2B: postflop raise caps, turn draw odds & pot control
├── sim/                             # Offline simulation & benchmarking engine
│   ├── mod.rs
│   ├── stats.rs                     # Match statistics, win/loss/tie ratios, standard error (SE)
│   ├── opponents.rs                 # Opponent archetype heuristics (TAG, Fish, Maniac, Rock, Self)
│   ├── holdem/                      # Texas Hold'em simulation & benchmark module
│   │   ├── mod.rs                   # Unified re-exports for benchmark, engine, and strategy
│   │   ├── strategy.rs              # Strategy model loading, infoset lookup, and blended fallbacks
│   │   ├── engine.rs                # Single-hand simulation engine with subgame resolving
│   │   └── benchmark/               # Duplicate Poker, stack-sweep, 6-max, and tournament benchmarks
│   │       ├── mod.rs
│   │       ├── hu.rs                # Heads-Up & variable-stack Duplicate Poker runner
│   │       ├── gauntlet.rs          # Archetype gauntlet suite
│   │       ├── multiway.rs          # 6-max multiway table simulator
│   │       ├── blind_schedule.rs    # Rising-BB tournament blind schedule evaluation
│   │       └── telemetry.rs         # Aggregate EV/mbb/CI & guard-trigger telemetry
│   └── leduc/                       # Leduc simulation & analytical evaluation
│       ├── mod.rs                   # Unified re-exports for evaluation and runner
│       ├── runner.rs                # Match simulator, spectator watch mode, and interactive CLI
│       └── eval.rs                  # Best response (NashConv), exploitability & random benchmarks
├── live/                            # Native real-time client for lil-poker web server
│   ├── mod.rs
│   ├── protocol.rs                  # REST/WebSocket schemas, card parsing, and pseudo-harmonic action mapping
│   ├── decision.rs                  # Real-time action decision engine with subgame resolving
│   ├── round_tracker.rs             # Multi-street betting round state tracking
│   ├── state/                       # Live table snapshot parsing & multiway table tracker
│   │   ├── mod.rs
│   │   ├── snapshot.rs              # Server payload parsing & legal-action derivation
│   │   └── table_tracker.rs         # HU/6-max opponent action & showdown tracking
│   └── client/                      # Live session transport & execution safety
│       ├── mod.rs                   # LiveBotSession async WebSocket event loop, submission & retry orchestration
│       ├── transport.rs             # Strategy loader, guest auth, room join & WebSocket connection
│       ├── commit_guards.rs         # Live stack-commit & postflop air stack-risk execution guards
│       └── fallback.rs              # Non-escalating rejected-action & emergency call fallback candidate builders
└── bin/
    ├── play_live.rs                 # CLI: Live bot client for lil-poker server
    ├── play.rs                      # CLI: Offline interactive & benchmark game simulator
    ├── analyze_model.rs             # CLI: Inspect trained strategy stats, street mix & infoset keys
    ├── distill_model.rs             # CLI: Prune & distill large blueprint models for low-RAM live containers
    ├── train_holdem.rs              # CLI: Train Texas Hold'em MCCFR/DCFR model
    ├── train_vanilla.rs             # CLI: Train exact full-tree Leduc solver
    └── evaluate.rs                  # CLI: Evaluate strategies (win rate, stack-sweep, blind-schedule, exploitability)
tests/
├── test_abstraction.rs              # Board texture, draw detection, and infoset key tests
├── test_cfr.rs                      # CFR+ regret clamping and DCFR discounting tests
├── test_eval.rs & eval/             # Bitwise 7-card evaluator tiebreakers & 88 abstraction buckets
├── test_fallback.rs                 # Preflop and postflop GTO fallback strategy bounds
├── test_game.rs                     # Hold'em game transitions, bet sizing, and duplicate dealing
├── test_live.rs & live/             # Live protocol, state snapshot/tracker, decision, and fallback tests
├── test_opponent.rs & opponent/     # Opponent metrics/styles, preflop/postflop/subgame guards, and exploit sizing
└── test_subgame.rs & subgame/       # Subgame reach distribution, adaptive budget, and solver tests
models/                              # Trained strategy models & checkpoints (.bin)
```

---

## Quick Start

### Build

```bash
cargo build --release
```

### Run Tests

```bash
cargo test --all-targets
```
*(All 129 integration tests across 8 test suites covering DCFR, bitwise evaluator tiebreakers, 88-bucket abstraction, explicit reach range subgame solving, pseudo-harmonic translation, live HU/6-max state tracking & non-escalating rejected-action fallbacks, draw detection, binary model serialization, Wilson intervals, short-stack Nash push/call defense, subgame safety guards, and opponent guards pass).*

---

### Training Texas Hold'em

Train a new Texas Hold'em blueprint model with parallel DCFR and regret-based pruning:

```bash
# Standard Deep Training (200M iterations, ~2.5 minutes on 16 threads)
cargo run --release --bin train_holdem -- \
  --iterations 200000000 \
  --threads 16 \
  --log-every 5000000 \
  --save-path models/holdem_abstract_strategy.bin

# Ultra High-Convergence Training (1.6 Billion iterations, ~20 minutes)
cargo run --release --bin train_holdem -- \
  --iterations 1600000000 \
  --threads 16 \
  --log-every 50000000 \
  --save-path models/holdem_abstract_strategy.bin
```

CLI options for `train_holdem`:
- `-i, --iterations <N>`: Total training iterations (default: `10000000`).
- `-t, --threads <N>`: Number of parallel Rayon threads (default: `16`).
- `-l, --log-every <N>`: Frequency of progress logging (default: `50000`).
- `-s, --save-path <PATH>`: Output path for strategy binary (default: `models/holdem_abstract_strategy.bin`).
- `--checkpoint-every <N>`: Save periodic checkpoints every `<N>` iterations (default: `0`, disabled).
- `--keep-checkpoints <bool>`: Keep versioned checkpoint files (default: `true`).
- `--rich-history <bool>`: Distinguish previous street action contexts (default: `true`).
- `--pruning <bool>`: Enable regret-based pruning (default: `true`).
- `--dcfr <bool>`: Enable Pluribus Discounted CFR (default: `true`).
- `--prune-river`: Prune river nodes on save for low-RAM runtimes (default: `false`).
- `--max-turn-actions <N>`: Limit turn actions kept on save, `0` = unlimited (default: `0`).
- `--resume <PATH>` / `--save-raw-ckpt`: Warm-start from an existing strategy or `.ckpt` raw solver state.
- `--stack-bb <N>`: Effective starting stack depth in Big Blinds when `--multi-stack` is false (default: `100`).
- `--multi-stack`: Train unified single-file multi-stack blueprint (`15 BB + 30 BB + 100 BB` in one model, default: `false`).
- `--board-texture`: Include compact 4-state board texture (`d`/`w`/`p`/`m`) in postflop infoset keys (default: `false`).
- `--short-stacks-only`: Train only `15 BB` (`S15|`) and `30 BB` (`S30|`) episodes while keeping `100 BB` nodes frozen (default: `false`).

---

### Model Distillation & Inspection

Prune large blueprint models down to compact runtime strategies for tight Docker RAM constraints (< 400 MB RAM limit):

```bash
cargo run --release --bin distill_model -- \
  models/holdem_abstract_strategy.bin \
  models/holdem_strategy_distilled.bin \
  --max-turn-actions 3 \
  --max-river-actions 2 \
  --max-pf-len 4
```
*(Pass `--prune-river` to drop all river nodes from the distilled binary when relying strictly on real-time subgame solving on the river.)*

Inspect street distribution, action frequencies, and specific infoset keys with `analyze_model`:

```bash
cargo run --release --bin analyze_model -- \
  models/holdem_abstract_strategy.bin \
  --pattern "P:AA" \
  --limit 20
```

---

### Play & Simulate Offline

Simulate bot performance against built-in opponent archetypes (`random`, `calling_station`, `maniac`, `rock`, `tag`, `self`):

```bash
# Simulate 100 hands with real-time subgame search against a Random opponent
cargo run --release --bin play -- \
  --game holdem \
  --hands 100 \
  --opponent random \
  --subgame-search

# Watch 10 interactive hands with action step delays
cargo run --release --bin play -- \
  --game holdem \
  --hands 10 \
  --delay 500 \
  --subgame-search
```

At the end of each session, a comprehensive HUD report is generated:
```
============================================================
        ♠️  Texas Hold'em: Bot Simulation Summary  ♥️        
============================================================
Opponent Archetype  : RANDOM
Total Hands Played  : 100
Total Net Profit    : +34,250.0 chips
Win Rate (BB/100)   : +1,712.50 bb/100
Win Rate (mbb/hand) : +17,125.0 mbb/hand
Opponent Style      : Maniac (Hyper-Aggressive) (Confidence: 85.0%)
============================================================
```

---

### Live Online Bot Client ([lil-poker](https://github.com/LilRaime/lil-poker) Server)

Connect the bot directly to a running [lil-poker](https://github.com/LilRaime/lil-poker) web server room (supports both HU and 6-max rooms; `--subgame-search` defaults to `true`):

```bash
cargo run --release --bin play_live -- \
  --url http://localhost:8090 \
  --room 4XMSSW \
  --name Rust_CFR_Bot \
  --subgame-search
```

### Docker Support

#### 1. Auto-spawn via lil-poker Web Server
Build the Docker image tagged as `lil-poker-mccfr` to allow the web server to spawn bot instances on demand:
```bash
docker build -t lil-poker-mccfr .
```

#### 2. Standalone Container
```bash
docker run --rm lil-poker-mccfr \
  --url http://host.docker.internal:8090 \
  --room 4XMSSW \
  --name Rust_CFR_Bot \
  --subgame-search
```

---

## Strategy Evaluation & Benchmarking

The unified `evaluate` CLI benchmarks trained models for both Texas Hold'em and Leduc Hold'em (additional options include `--stack-bb`, `--exploit-factor`, `--seed`, `--disable-guard`, and `--opponent ablate`):

```bash
# 1. Evaluate Texas Hold'em Blueprint (Duplicate Poker benchmark against TAG opponent with real-time subgame solving)
cargo run --release --bin evaluate -- \
  --game holdem \
  --hands 5000 \
  --opponent tag \
  --duplicate true \
  --subgame

# 2. Sweep across stack depths (15 BB, 30 BB, 100 BB) or rising-BB tournament blind schedules
cargo run --release --bin evaluate -- \
  --game holdem \
  --hands 1000 \
  --stack-sweep \
  --subgame

cargo run --release --bin evaluate -- \
  --game holdem \
  --hands 1000 \
  --blind-schedule \
  --level-hands 20 \
  --subgame

# 3. 7-Archetype Gauntlet & 6-Max Multiway Benchmarks
cargo run --release --bin evaluate -- \
  --game holdem \
  --hands 1000 \
  --opponent gauntlet \
  --subgame

cargo run --release --bin evaluate -- \
  --game holdem \
  --hands 1000 \
  --opponent 6max \
  --subgame

# 4. A/B Head-to-Head Testing Between Two Hold'em Strategy Profiles
cargo run --release --bin evaluate -- \
  --game holdem \
  --strategy models/holdem_abstract_strategy.bin \
  --opp-strategy models/holdem_strategy_distilled.bin \
  --hands 5000 \
  --duplicate true

# 5. Exact Exploitability / NashConv & Best Response Analysis (Leduc Hold'em)
cargo run --release --bin evaluate -- \
  --game leduc \
  --strategy models/leduc_strategy.json \
  --hands 500000
```

---

## Training Leduc Hold'em Toy Game

Leduc Hold'em serves as an analytical testbed with exact Best Response and exploitability validation:

```bash
# Parallel MCCFR (fast, ~30s)
cargo run --release --bin train -- --iterations 200000 --threads 8 --save-path models/leduc_strategy.json

# Exact Vanilla CFR+ (exact Nash equilibrium)
cargo run --release --bin train_vanilla -- --iterations 5000 --save-path models/leduc_vanilla.json
```

---

## License

This project is licensed under the **GNU General Public License v3.0 or later (GPL-3.0-or-later)**.
