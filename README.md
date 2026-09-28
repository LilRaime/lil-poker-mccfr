# lil-poker-mccfr 🃏

A high-performance parallel poker solver, online subgame resolver, and exploitative bot client written in Rust. Implements **External Sampling MCCFR with Discounted CFR (DCFR)**, **CFR+**, and **Real-Time Subgame Search (Pluribus/Libratus-style)** for both 52-card Texas Hold'em and Leduc Hold'em.

Powered by an ultra-fast **$O(1)$ bitwise 7-card hand evaluator** capable of sustaining **> 1,300,000+ iterations/second** on consumer multicore CPUs.

---

## Key Features

- **Blazing Fast Performance ($> 1.3\text{M}$ iter/s)** — Pure $O(1)$ bitwise 7-card evaluator (zero-alloc, zero-sort, bitmask straights/flushes, x86 hardware `LZCNT` kickers) + lock-free Rayon & 1024-shard DashMap parallelism.
- **Discounted CFR (DCFR, Pluribus algorithm)** — Analytical regret & strategy discount schedules ($\alpha=1.5, \beta=0.5, \gamma=2.0$) that clear early-iteration noise and converge dramatically faster than vanilla CFR+.
- **Real-Time Subgame Search with Adaptive Budgeting** — Depth-limited Turn & River subgame resolver that dynamically scales iteration depth based on pot size, street, and bet pressure (from 500 up to 6,000+ iterations).
- **Safe Resolving (Burch et al. 2014)** — Blends real-time subgame policies with robust GTO fallback bounds (85% solved / 15% fallback) to prevent gift-giving and opponent out-of-distribution exploitation.
- **Texture-Aware & Potential Abstraction** — 169 canonical preflop hand groups + 50 postflop equity buckets conditioned on board texture (Monotone, TwoTone, Rainbow, paired/connected boards) and combo draw potential (NFD, OESD + overcards).
- **Regret-Based Pruning (RBP)** — Skips 95% of deeply negative regret branches (`regret < -300`), accelerating MCCFR traversals by 2–3x without equilibrium distortion.
- **Exploitative Opponent Modeling & Style Classifier** — Tracks VPIP, PFR, 3-bet frequency, aggression frequency, and classifies opponents into archetypes (`CallingStation`, `Maniac`, `Rock`, `TAG`, `LAG`).
- **Showdown Learning** — Detects opponent bluffing and trapping habits from revealed hole cards at showdown, dynamically tuning bluff-catching frequencies.
- **Geometric Street-Aware Bet Sizing** — Dynamic pot-scaled sizing: 50% pot on dry flop, 75% pot on wet flop/turn, 100% full pot on river.
- **Purified Action Defense** — Clamps Monte Carlo noise and prevents accidental folds of premium pocket pairs (AA/KK/QQ) when facing all-in shoves.
- **Live Server Bot Client (`play_live`)** — Native async WebSocket & REST client connecting directly to [lil-poker](https://github.com/LilRaime/lil-poker) web rooms.
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
* **DCFR (Brown & Sandholm 2019):** Uses polynomial weighting:
  $$\text{Positive regret discount: } \frac{t^\alpha}{t^\alpha + 1} \quad (\alpha = 1.5)$$
  $$\text{Negative regret discount: } \frac{t^\beta}{t^\beta + 1} \quad (\beta = 0.5)$$
  $$\text{Strategy contribution weight: } \left(\frac{t}{t+1}\right)^\gamma \quad (\gamma = 2.0)$$
  This eliminates early exploration artifacts without requiring heuristic restart phases.

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
At Turn and River nodes, the bot constructs a localized subgame conditioned on Bayesian opponent reach ranges. The iteration budget scales dynamically:
$$\text{Budget} = \text{Base} \times M_{\text{pot}} \times M_{\text{street}} \times M_{\text{pressure}}$$
* Tiny pots ($\le 60$ chips): $0.55\times$ base for instant millisecond responses.
* Huge pots ($\ge 800$ chips): up to $2.2\times$ base for deep precision.
* River nodes: $1.35\times$ boost (terminal street with zero chance nodes).
* Facing large bet / all-in ($\ge 100$ chips): $1.25\times$ boost.

---

## Project Structure

```
src/
├── lib.rs
├── main.rs                  # CLI: Leduc Hold'em MCCFR training
├── game/
│   ├── card.rs              # 6-card Leduc & 52-card Hold'em deck definitions
│   ├── leduc.rs             # Leduc Hold'em rules & state transitions
│   └── holdem.rs            # Texas Hold'em game engine & O(1) bitwise evaluator
├── cfr/
│   ├── mod.rs
│   ├── node.rs              # Lock-free InfosetNode (AtomicI64, CFR+ & DCFR)
│   ├── holdem_mccfr.rs      # Multi-threaded Hold'em MCCFR solver with DCFR & RBP
│   ├── mccfr.rs             # Leduc MCCFR solver
│   ├── vanilla.rs           # Exact full-tree Vanilla CFR+ solver (Leduc)
│   ├── abstraction.rs       # Card abstraction (texture detection, draws, equity buckets)
│   ├── fallback.rs          # Robust GTO fallback bounds for safe resolving
│   ├── subgame.rs           # Real-Time Subgame Solver (adaptive budgeting & safe resolving)
│   └── opponent_model.rs    # Opponent tracker, style classifier, and purified defense
└── bin/
    ├── play_live.rs         # Live WebSocket/REST bot client for lil-poker server
    ├── train_holdem.rs      # CLI: Train Texas Hold'em MCCFR/DCFR model
    ├── train_vanilla.rs     # CLI: Train exact full-tree Leduc solver
    ├── evaluate.rs          # CLI: Evaluate strategies (win rate, exploitability)
    └── play.rs              # CLI: Play / simulate games offline with rich analytics
tests/
└── solver_tests.rs          # 21 comprehensive unit tests (GTO, DCFR, evaluator, subgame)
models/
├── leduc_strategy.json      # Pre-trained Leduc strategy
└── holdem_abstract_strategy.json  # Pre-trained Hold'em blueprint model
```

---

## Quick Start

### Build

```bash
cargo build --release
```

### Run Tests

```bash
cargo test
```
*(All 21 unit tests covering DCFR, bitwise evaluator tiebreakers, subgame solving, draw detection, and opponent modeling pass in < 0.05s).*

---

### Training Texas Hold'em

Train a new Texas Hold'em blueprint model with parallel DCFR and regret-based pruning:

```bash
# Standard Deep Training (200M iterations, ~2.5 minutes on 16 threads)
cargo run --release --bin train_holdem -- \
  --iterations 200000000 \
  --threads 16 \
  --log-every 5000000 \
  --save-path models/holdem_abstract_strategy.json

# Ultra High-Convergence Training (1.6 Billion iterations, ~20 minutes)
cargo run --release --bin train_holdem -- \
  --iterations 1600000000 \
  --threads 16 \
  --log-every 50000000 \
  --save-path models/holdem_abstract_strategy.json
```

CLI options for `train_holdem`:
- `-i, --iterations <N>`: Total training iterations (default: `10000000`).
- `-t, --threads <N>`: Number of parallel Rayon threads (default: `16`).
- `-l, --log-every <N>`: Frequency of progress logging (default: `50000`).
- `-s, --save-path <PATH>`: Output path for strategy JSON (default: `models/holdem_abstract_strategy.json`).
- `--dcfr`: Enable Pluribus Discounted CFR (default: `true`).
- `--pruning`: Enable regret-based pruning (default: `true`).
- `--rich-history`: Distinguish previous street action contexts (default: `false`).

---

### Play & Simulate Offline

Simulate bot performance against built-in opponent archetypes (`RANDOM`, `CALLING_STATION`, `MANIAC`, `ROCK`, `TAG`):

```bash
# Simulate 100 hands with real-time subgame search against a Random opponent
cargo run --release --bin play -- \
  --game holdem \
  --hands 100 \
  --opp-archetype RANDOM \
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

Connect the bot directly to a running [lil-poker](https://github.com/LilRaime/lil-poker) web server room:

```bash
cargo run --release --bin play_live -- \
  --url http://localhost:8090 \
  --room 4XMSSW \
  --name CFR_Pluribus_Bot \
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

## Training Leduc Hold'em Toy Game

Leduc Hold'em serves as an analytical testbed with exact Best Response and exploitability validation:

```bash
# Parallel MCCFR (fast, ~30s)
cargo run --release -- --iterations 200000 --threads 8 --save-path models/leduc_strategy.json

# Exact Vanilla CFR+ (exact Nash equilibrium)
cargo run --release --bin train_vanilla -- --iterations 5000 --save-path models/leduc_vanilla.json

# Evaluate Exploitability / NashConv
cargo run --release --bin evaluate -- --strategy models/leduc_strategy.json --hands 500000
```

---

## License

This project is licensed under the **GNU General Public License v3 (GPL-3.0)**.
