# lil-poker-mccfr 🃏

A parallel poker solver and simulator written in Rust, implementing **External Sampling MCCFR with CFR+** for both Leduc Hold'em and 52-card Texas Hold'em.

## Features

- **Two game variants** — Leduc Hold'em (6-card toy game) and full 52-card Texas Hold'em
- **Parallel MCCFR (CFR+)** — lock-free multi-threaded training via Rayon + DashMap
- **Real-time Subgame Search** — depth-limited subgame solver for Turn & River online strategy refining
- **Opponent Modeling** — real-time opponent tracking (VPIP, PFR, Aggression Frequency)
- **Live Server Bot Client (`play_live`)** — native async Rust client connecting to [lil-poker](https://github.com/LilRaime/lil-poker) server via WebSockets and REST API
- **Card Abstraction** — 169 preflop buckets + 50 postflop equity buckets for Hold'em
- **Vanilla CFR+** — exact full-tree solver for Leduc (for validation / comparison)
- **Exploitability** — exact Best Response / NashConv computation for Leduc
- **Interactive CLI** — watch bot vs random, play episodes, or (Leduc) human vs bot

---

## Algorithm

### Counterfactual Regret Minimization (CFR)

CFR is the standard algorithm for finding Nash Equilibria in imperfect-information games (poker). The average strategy of both players converges to a Nash Equilibrium as iterations → ∞.

**CFR+** variant: regret sums are clamped to 0 (floor), which significantly accelerates convergence in practice.

### External Sampling MCCFR

Instead of traversing the full game tree (expensive for Hold'em), External Sampling MCCFR:
- **Updating player** — explores all legal actions, computes counterfactual regrets
- **Opponent** — samples a single action according to current strategy

This reduces per-iteration cost from `O(|tree|)` to `O(|updating_player_subtree|)`.

### Real-Time Subgame Resolving

During live play (`play_live`), on postflop streets (Turn & River), the bot constructs a localized subgame rooted at the current public state and runs 1500 iterations of subgame MCCFR resolving to refine the abstract strategy for the specific board and action history.

### Parallelism

Each Rayon thread runs independent traversals. The shared infoset table uses:
- **`DashMap<String, Arc<InfosetNode>>`** — concurrent hash map with shard-level locking
- **`AtomicI64` with `fetch_add`** — lock-free regret/strategy accumulation (no Mutex, no CAS spinning)
- Fixed-point scaling (`× 1_000_000`) to store `f64` values as integers

### Card Abstraction (Hold'em)

The full 52-card Hold'em game tree is intractably large. We reduce it via:

| Street | Abstraction | Buckets |
|--------|-------------|---------|
| Preflop | Canonical hand group (pair/suited/offsuit) | 169 |
| Flop / Turn / River | Hand category + kicker rank | 50 |

Examples: `AA`, `AKs`, `AKo` (preflop) · `F:B42/rc` (postflop, flush bucket 42, action sequence raise-call)

---

## Project Structure

```
src/
├── lib.rs
├── main.rs                  # train binary (Leduc MCCFR)
├── game/
│   ├── card.rs              # 6-card Leduc deck & 52-card Hold'em deck
│   ├── leduc.rs             # Leduc Hold'em game engine
│   └── holdem.rs            # 52-card Texas Hold'em + 7-card evaluator
├── cfr/
│   ├── mod.rs
│   ├── node.rs              # Lock-free InfosetNode (AtomicI64)
│   ├── mccfr.rs             # Parallel MCCFR for Leduc
│   ├── holdem_mccfr.rs      # Parallel MCCFR for Hold'em
│   ├── vanilla.rs           # Exact full-tree Vanilla CFR+
│   ├── abstraction.rs       # Card abstraction (preflop buckets + equity buckets)
│   ├── subgame.rs           # Real-time Subgame Solver (Turn & River resolving)
│   └── opponent_model.rs    # Opponent VPIP/PFR/Aggression tracker
└── bin/
    ├── play_live.rs         # Live Rust bot client for lil-poker server
    ├── train_holdem.rs      # CLI: train Hold'em model
    ├── train_vanilla.rs     # CLI: train exact Leduc solver
    ├── evaluate.rs          # CLI: evaluate Leduc strategy (exploitability, win rate)
    └── play.rs              # CLI: play / watch / simulate games offline
models/
├── leduc_strategy.json      # Pre-trained Leduc MCCFR strategy
├── leduc_vanilla.json       # Pre-trained Leduc Vanilla CFR+ strategy
└── holdem_abstract_strategy.json  # Pre-trained Hold'em model (50M iterations)
```

---

## Quick Start

### Build

```bash
cargo build --release
```

### Live Bot Client ([lil-poker](https://github.com/LilRaime/lil-poker) Server)

Play against human or bot opponents on a running [lil-poker](https://github.com/LilRaime/lil-poker) web server:

```bash
# Connect live Rust bot client to local lil-poker room with subgame search
cargo run --release --bin play_live -- \
  --url http://localhost:8090 \
  --room 4XMSSW \
  --name CFR_Bot \
  --subgame-search
```

### Docker

#### 1. Integration with `lil-poker` (Auto-spawn by Web Server)

To allow the `lil-poker` web server to spawn this Rust bot automatically when you click "+ MCCFR Bot" in the web UI, build the Docker image with the tag `lil-poker-mccfr`:

```bash
docker build -t lil-poker-mccfr .
```

#### 2. Standalone Docker Run

```bash
# Build image
docker build -t lil-poker-mccfr .

# Run bot container manually
docker run --rm lil-poker-mccfr \
  --url http://host.docker.internal:8090 \
  --room 4XMSSW \
  --name Rust_CFR_Bot \
  --subgame-search
```

### Train — Leduc Hold'em (fast, ~30s)

```bash
# Parallel MCCFR (default)
cargo run --release -- --iterations 200000 --threads 8 --save-path models/leduc_strategy.json

# Exact Vanilla CFR+ (slower, exact Nash)
cargo run --release --bin train_vanilla -- --iterations 5000 --save-path models/leduc_vanilla.json
```

### Train — Texas Hold'em 52-card (~40 min)

```bash
cargo run --release --bin train_holdem -- \
  --iterations 50000000 \
  --threads 16 \
  --log-every 1000000 \
  --save-path models/holdem_abstract_strategy.json
```

Progress output:
```
  [ 1000000/50000000 |   2.0%]  infosets= 577600  speed=12k/s  elapsed=1m26s  eta=1h10m
  [ 5000000/50000000 |  10.0%]  infosets= 780000  speed=19k/s  elapsed=5m10s  eta=37m
  [50000000/50000000 | 100.0%]  infosets= 879487  speed=21k/s  elapsed=41m04s  eta=0s
```

### Play / Watch

```bash
# Watch 10 hands of Hold'em (bot vs random opponent)
cargo run --release --bin play -- --game holdem --hands 10

# Watch 10 hands of Leduc (bot vs random, shows infoset + probabilities)
cargo run --release --bin play -- --game leduc --mode watch --hands 10

# Human vs Nash Bot (Leduc)
cargo run --release --bin play -- --game leduc --mode human --hands 5
```

### Evaluate (Leduc)

```bash
cargo run --release --bin evaluate -- \
  --strategy models/leduc_strategy.json \
  --hands 500000 \
  --verbose
```

Example output:
```
Player 0 (OOP):  Win Rate: +312.4 mbb/hand
Player 1 (IP):   Win Rate: +287.1 mbb/hand

Exploitability : 0.023 chips/hand
Status: ✅ EXCELLENT — Near Nash Equilibrium (< 0.05)
```

---

## Results

### Leduc Hold'em (200k iterations, ~30s)

| Metric | Value |
|--------|-------|
| Infosets | ~300 |
| Exploitability | < 0.05 chips/hand |
| Status | Near Nash Equilibrium |

### Texas Hold'em (50M iterations, ~41 min, Ryzen 7 7735HS / 16 threads)

| Metric | Value |
|--------|-------|
| Infosets | ~880,000 |
| Training speed (warmed up) | ~20–23k iter/s |
| Peak speed | 23k iter/s |

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| `rayon` | Data-parallel iteration across CPU cores |
| `dashmap` | Concurrent shard-locked hash map |
| `rand` / `SmallRng` | Fast per-thread PRNG |
| `serde` / `serde_json` | Strategy serialization |
| `clap` | CLI argument parsing |

---

## Design Notes

### Why `fetch_add` instead of CAS?

Popular infosets (e.g. `AKs` preflop) are accessed by all threads simultaneously. A compare-and-swap loop would spin 10–50 times per update under high contention. `fetch_add` is a single locked atomic instruction that always succeeds. The CFR+ clamp (floor at 0) is applied lazily at read time in `get_strategy()` — semantically equivalent.

### Why fixed-point integers?

`AtomicF64` does not exist in stable Rust. We scale `f64` by `1_000_000` and store as `AtomicI64`, providing ~6 decimal digits of precision — sufficient for regret accumulation over millions of iterations.

### Why card abstraction for Hold'em?

The full 52-card Hold'em tree has ~10¹⁸ game states. Card abstraction reduces the infoset space to ~880k nodes, making training feasible in under an hour on consumer hardware. The tradeoff: hands that fall into the same equity bucket are treated identically (e.g. a strong flush and a weak flush in the same bucket may receive the same strategy).

---

## License

This project is licensed under the **GNU General Public License v3 (GPL-3.0)**.
