# lil-poker-mccfr 🃏

A parallel poker solver, real-time depth-limited subgame resolver, and exploitative live bot client written in Rust (Edition 2024). Supports 52-card Texas Hold'em (Heads-Up and 6-max) and Leduc Hold'em using External Sampling MCCFR with Pluribus Discounted CFR (DCFR).

## Key Features

- **Parallel MCCFR with DCFR & Regret-Based Pruning** — Lock-free Structure-of-Arrays regret/strategy tables, incremental FNV-1a traversal state, L3-resident preflop slot caching, and epoch-based Pluribus DCFR discounting with RBP pruning.
- **$O(1)$ Bitwise 7-Card Evaluator & Compile-Time LUTs** — Zero-allocation vertical bitboard hand evaluation, SWAR suit detection, hardware `leading_zeros()` kickers, and L1 rank-mask lookup tables for straights, draws, and board textures.
- **Real-Time Subgame Search & Safe Resolving** — Depth-limited Turn, River, and Flop subgame solver with Bayesian `OpponentReachDistribution`, dynamic pot/street/pressure iteration budgeting, GTO fallback blending, and pseudo-harmonic action translation.
- **Texture & Unified Multi-Stack Abstraction** — 169 canonical preflop buckets (`0..=168`) and 88 equity/draw postflop buckets (`0..=87`) with optional 4-state flop texture keys and single-file multi-stack training (`15 BB` / `30 BB` / `100 BB`).
- **Opponent Modeling & Priority Guard Pipeline** — Tracks VPIP, PFR, 3-bet, c-bet, barrel, and showdown tells with Wilson intervals to drive style classification, short-stack Nash push/fold, exploitative sizing, and multi-tier safety guards.
- **Live Server Client & Exact Leduc Testbed** — Async WebSocket/REST client (`play_live`) for [lil-poker](https://github.com/LilRaime/lil-poker) HU/6-max rooms with execution commit guards and non-escalating rejected-action fallbacks, plus exact full-tree Vanilla CFR+ and NashConv analysis for Leduc Hold'em.

## Documentation

- **[Algorithms & Engine Internals](docs/algorithms.md)** — MCCFR/DCFR/RBP math, lock-free training layout, PGO build recipe, $O(1)$ bitwise evaluator & L1 LUTs, card abstraction, subgame resolving, and action translation.
- **[Architecture, Live Pipeline & CLI Reference](docs/architecture.md)** — Subsystem overview, live decision & priority guard flow, model formats/distillation, Docker support, and complete CLI flags across all binaries.

## Quick Start

### Build & Run Tests

```bash
cargo build --release
cargo test --all-targets
```

Runs **136** integration tests across 8 suites covering CFR+/DCFR, `TrainState` & LUT parity, 88-bucket abstraction, subgame reach/solving, live protocol & fallback guards, and opponent modeling.

### Train a Hold'em Blueprint

```bash
cargo run --release --bin train_holdem -- \
  --iterations 200000000 \
  --threads 16 \
  --log-every 5000000 \
  --multi-stack \
  --board-texture \
  --save-raw-ckpt \
  --save-path models/holdem_abstract_strategy.bin
```

See [Architecture & CLI Reference](docs/architecture.md) for all `train_holdem`, `analyze_model`, and Leduc training flags, and [Algorithms & Engine Internals](docs/algorithms.md) for optional local PGO builds.

### Connect to a Live Server (`play_live` & Docker)

```bash
cargo run --release --bin play_live -- \
  --url http://localhost:8090 \
  --room 4XMSSW \
  --name Rust_CFR_Bot \
  --subgame-search
```

Or run via Docker:

```bash
docker build -t lil-poker-mccfr .
docker run --rm lil-poker-mccfr --url http://host.docker.internal:8090 --room 4XMSSW --name Rust_CFR_Bot --subgame-search
```

### Evaluate & Distill Models

```bash
# Duplicate Hold'em Gauntlet benchmark with real-time subgame solving
cargo run --release --bin evaluate -- \
  --game holdem \
  --hands 1000 \
  --opponent gauntlet \
  --subgame

# Distill a blueprint model for low-RAM Docker containers
cargo run --release --bin distill_model -- \
  models/holdem_abstract_strategy.bin \
  models/holdem_strategy_distilled.bin \
  --max-turn-actions 3 \
  --max-river-actions 3 \
  --max-pf-len 4 \
  --prune-river
```

## License

This project is licensed under the **GNU General Public License v3.0 or later (GPL-3.0-or-later)**.
