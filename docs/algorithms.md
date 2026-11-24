# Algorithms & Engine Internals

Technical reference for the solving algorithms, lock-free training architecture, $O(1)$ bitwise evaluator, abstraction pipeline, and real-time subgame search in `lil-poker-mccfr`.

> See [Architecture & CLI Reference](architecture.md) for system layout, live guard pipelines, and CLI options, or return to the [README](../README.md).

---

## 1. Counterfactual Regret Minimization (MCCFR, DCFR & RBP)

The Texas Hold'em blueprint solver (`src/cfr/holdem_mccfr.rs`) implements **Parallel External Sampling MCCFR** with **Discounted CFR (DCFR)** and **Regret-Based Pruning (RBP)**.

### Paired External Sampling
* In each sampled episode, nine non-repeating cards (2 hole cards per player + 5 board runout cards) are dealt via a single 64-bit bitmask (`deal_9_cards` in `src/cfr/train_state.rs`).
* Whenever possible, `P0` and `P1` updates share the same `EpisodeCtx` (precomputed buckets, flop texture, and lazy showdown cache), eliminating redundant card deals and repeated 7-card evaluations across paired traversals.
* During traversal of the updating player's nodes, all non-pruned legal actions are explored and atomic regret sums are updated (`f64` utilities scaled by `SCALE_F64 = 1_000_000.0` into `AtomicI64`); at opponent nodes, a single action is sampled proportionally to current regret-matched probabilities while accumulating weighted average strategy sums.

### Discounted CFR (Pluribus Schedule)
When `--dcfr` is enabled (the default), the solver applies epoch-based polynomial discounting (Brown & Sandholm 2019) across all visited infosets every `dcfr_epoch = (iterations / 20).clamp(50_000, 10_000_000)` iterations:

$$\text{Positive regret discount} = \frac{t^\alpha}{t^\alpha + 1}, \qquad \alpha = 1.5$$

$$\text{Negative regret discount} = \max\!\left(\frac{t}{t + 1},\, 0.75\right)$$

$$\text{Strategy sum discount} = \left(\frac{t}{t + 1}\right)^\gamma, \qquad \gamma = 2.0$$

This clears early-iteration variance without needing hard warmup resets (when `--dcfr false` is passed, classic linear CFR+ weighting with non-negative regret clamping is used instead).

### Regret-Based Pruning (RBP)
When `--pruning` and `--dcfr` are active past the warmup window (`(total_target / 20).clamp(1_000, 500_000)` iterations), RBP stochastically skips 95% of deeply negative branches:
* Threshold: $-\max(3.5 \times \text{pot},\, 0.06 \times \text{stack\_size})$, compared directly in integer-scaled regret units (`raw_regrets[i] < r_thresh_scaled`) alongside `legal_probs[i] < 0.005`.
* **Never pruned**: `FOLD`, any `ROUND_RIVER` action, or `CALL_CHECK` facing an opponent all-in (`contributions[opp] >= stack_size`).

---

## 2. Lock-Free Training Data Structures

All blueprint training optimizations are **lossless**—hand evaluation, abstraction buckets, infoset keys/hashes, `f64` utilities, and `.bin` / `.ckpt` formats remain bit-compatible.

### Structure-of-Arrays `FlatInfosetTable` (`src/cfr/flat_table.rs`)
* **Cache-Line Separation**: Splits hot training arrays into independent 64-byte aligned arrays: `regret_slots: [RegretSlot]` (`hash: AtomicU64`, `regret_sum: [AtomicI64; 6]`) and `strat_slots: [StratSlot]` (`strategy_sum: [AtomicI64; 6]`), avoiding false cache-line sharing and keeping unused strategy bytes out of updating-player traversal cache lines.
* **Open Addressing + Overflow**: Zero-page lazy-allocated flat slots probed linearly up to `MAX_PROBE`, with `overflow: DashMap<u64, InfosetEntry>` as an append-only fallback for rare probe saturation.
* **Sharded Discounting**: New slot indices are appended to lock-sharded vectors (`occupied_shards`), allowing Rayon threads to discount only active slots during DCFR epochs.
* **Cold Key Storage**: String infoset keys (`P:...`, `S15|...`) are stored out-of-band in `keys: DashMap<usize, (u64, String)>` for serialization and diagnostics, never touched on hit paths.

### Compact `TrainState` & L3 Preflop Slot Cache (`src/cfr/train_state.rs`)
* **Value-Type Traversal State**: `TrainState` fits into compact copy-by-value fields tracking contributions, active round, raises, incremental FNV-1a street `prefix_hash`, and bit-packed preflop/flop coarse action histories (`u32`), avoiding heap allocations during recursion.
* **Shared River Evaluation**: `EpisodeCtx` caches `river_scores: [Cell<u64>; 2]` and shares them between `ROUND_RIVER` equity bucketing (`postflop_equity_bucket_with_score`) and terminal showdown resolution.
* **Collision-Free Base-5 Preflop Slot Cache**: Sequences of up to 4 preflop actions encode bijectively into `pf_seq` (`0..781`). `FlatInfosetTable::get_or_insert_preflop` caches resolved flat indices in `pf_slot_cache: Box<[AtomicU32]>` across `3 × 169 × 781` entries (~1.5 MB, L3-resident on modern desktop/laptop CPUs), bypassing FNV-1a hashing and open-addressing probes on repeated preflop visits while populating identically from `.ckpt` warm-starts.

### Optional Profile-Guided Optimization (PGO) Build
For multi-hour training runs, a local PGO + `-Ctarget-cpu=native` build of `train_holdem` optimizes branch layout using runtime profile counters:

```bash
rustup component add llvm-tools-preview

rm -rf /tmp/pgo-data /tmp/pgo_warmup*
RUSTFLAGS="-Ctarget-cpu=native -Cprofile-generate=/tmp/pgo-data" cargo build --release --bin train_holdem

./target/release/train_holdem -i 25000000 --threads 16 --log-every 25000000 \
  --multi-stack --board-texture --resume models/holdem_v2.bin.ckpt --save-path /tmp/pgo_warmup.bin

"$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-profdata" merge \
  -o /tmp/pgo-data/merged.profdata /tmp/pgo-data

RUSTFLAGS="-Ctarget-cpu=native -Cprofile-use=/tmp/pgo-data/merged.profdata" \
  cargo build --release --bin train_holdem
```

---

## 3. $O(1)$ Bitwise 7-Card Evaluator & Compile-Time L1 LUTs

### Vertical Bitboard 7-Card Evaluator (`src/game/holdem/eval.rs`)
`evaluate_7cards` evaluates 5-, 6-, and 7-card hands with zero heap allocations and zero sorting:
* **Vertical Bitboard Rank Adder**: Uses four 16-bit rank bitmasks (`m1`, `m2`, `m3`, `m4`) updated in registers per card (`m4 |= m3 & bit; m3 |= m2 & bit; m2 |= m1 & bit; m1 |= bit`) to detect quads, trips, two-pair, and one-pair without `[u8; 13]` count histograms.
* **SWAR Suit Detection**: Packs all 4 suit counts into a single `u32` and detects a 5+ card flush via carry bit arithmetic: `((suit_counts_packed + 0x0303_0303) & 0x0808_0808) != 0`.
* **L1 Straight LUT & Hardware Kickers**: `STRAIGHT_HIGH_LUT: [u8; 8192]` (`8 KB` compile-time table, covering ace-high through ace-low `0x100F` wheel straights) resolves 5-card straights and straight flushes in one indexed load; top ranks and kickers are extracted with `15 - mask.leading_zeros()` (`LZCNT`/`BSR`).

### Single-Pass Board & Draw Lookup Tables
* **Board Texture & Span Summary** (`src/cfr/abstraction/texture.rs`, `src/cfr/abstraction/postflop/pairs.rs`): `BOARD_SPAN_LUT: [u8; 8192]` (`8 KB`) precomputes connected (`FLAG_CONNECTED`), 4-to-straight (`FLAG_FOUR_STRAIGHT`), and 3-in-4-span (`FLAG_THREE_IN_FOUR`) flags. `compute_board_summary` pairs this with SWAR suit packing (`0x0505_0505` for 3-flush, `0x0404_0404` for 4-flush) so pair/two-pair bucketing analyzes the board in a single pass.
* **Draw & Backdoor LUTs** (`src/cfr/abstraction/draws.rs`, `src/cfr/abstraction/postflop/high_card.rs`): `OESD_WIN_LUT`, `GUTSHOT_WIN_LUT`, and `BDSD_WIN_LUT` (`16 KB` each) encode straight-draw windows per 13-bit rank mask, restricting checks strictly to windows containing at least one non-board hole card.

---

## 4. Card Abstraction & Geometric Bet Sizing

### Preflop & Postflop Bucketing
* **Preflop (`169` buckets, `0..=168`)**: Canonicalized into 13 pocket pairs, 78 suited combinations, and 78 offsuit combinations (`src/cfr/abstraction/preflop.rs`), grouped into 8 strength tiers (`1..=8`) with precomputed short-stack Nash push/call tables.
* **Postflop (`88` equity buckets, `0..=87`)**: Category-aware bucketing (`src/cfr/abstraction/postflop/`) separating kicker strength, overpairs vs underpairs, paired-board holdings, board trips vs pocket sets, idiot-end/board straights, nut/second-nut flushes, full houses, and draw/combo/redraw equity (OESD, gutshot, flush draw, NFD, flop BDFD/BDSD).
* **Optional Blueprint Keys (`--board-texture` & `--multi-stack`)**:
  * `--board-texture` appends compact 4-state postflop flop texture codes (`m` monotone, `p` paired, `w` wet two-tone connected, `d` dry).
  * `--multi-stack` interleaves `15 BB` (`S15|`), `30 BB` (`S30|`), and `100 BB` (default unprefixed) episodes (`25% / 25% / 50%` schedule) in one binary, allowing short-stack repair passes via `--short-stacks-only`.

### Geometric Street-Aware Bet Sizing (`src/game/holdem/actions.rs`, `src/game/holdem/state.rs`)
Actions use discrete index codes `FOLD = 0`, `CALL_CHECK = 1`, `RAISE_MIN = 2`, `RAISE_THIRD_POT = 3`, `RAISE_HALF_POT = 4`, `ALL_IN = 5`:
* Preflop opens size around `2.5x BB`, dry flops ~50% pot, wet flops/turns ~75% pot, and river `RAISE_HALF_POT` scales to ~125% polarized overbets.
* `calculate_geometric_bet_size` computes stack-committing multipliers across `streets_left`:

$$\text{bet\_fraction} = \operatorname{clamp}\!\left(\frac{(1 + 2 \cdot \text{SPR})^{1/\text{streets\_left}} - 1}{2},\, 0.20,\, 2.0\right)$$

---

## 5. Real-Time Subgame Search, Safe Resolving & Action Translation

### Depth-Limited Subgame Solver (`src/cfr/subgame/`)
At `ROUND_TURN`, `ROUND_RIVER`, and medium/large `ROUND_FLOP` pots ($\ge 8\text{ BB}$ in Heads-Up play), `SubgameSolver` solves a localized depth-limited CFR+ tree:
* **Bayesian Opponent Reach Ranges**: `OpponentReachDistribution` initializes priors from preflop action lines (limp, open, 3-bet) adapted by observed VPIP/PFR and updates weights across postflop streets, textures, and bet sizes.
* **Symmetry & Anti-Leakage**: When updating Hero, root hands alternate between exact private hole cards and `hero_reach`; when updating Opponent, Hero hole cards are always sampled from `hero_reach` so Opponent cannot learn a clairvoyant counter-strategy.
* **Adaptive Iteration Budget**:

$$\text{Budget} = \text{Base} \times M_{\text{pot}} \times M_{\text{street}} \times M_{\text{pressure}}$$

| Factor | Condition | Multiplier |
| :--- | :--- | :---: |
| Pot $\le 6\text{ BB}$ | Small pot (`SUBGAME_POT_XS`) | $0.55\times$ |
| Pot $\ge 20\% / 40\% / 80\%$ stack | `SUBGAME_POT_SM` / `MD` / `LG` | $1.25\times$ / $1.6\times$ / $2.2\times$ |
| Street | `ROUND_TURN` / `ROUND_RIVER` | $1.1\times$ / $1.35\times$ |
| Bet pressure | `to_call >= 10 BB` (`SUBGAME_BET_PRESSURE`) | $1.25\times$ |

Results are clamped to `[max(100, base / 3).min(base), max(base * 3, 6000)]`.

### Safe Resolving (Burch et al. 2014)
To prevent out-of-distribution exploitation, `finalize_root_strategy` blends subgame CFR+ policies with GTO-aligned heuristic fallbacks (`src/cfr/fallback/heuristic.rs`):
* **Standard made hands & strong draws**: `88%` subgame solved / `12%` GTO fallback.
* **Air & weak board-pair holdings** (`bucket < BUCKET_STRONG_DRAW_MIN` or `bucket` `28..=29`): zeroes `ALL_IN` (and all raises when facing a bet), then blends `65%` solved / `35%` fallback when `to_call == 0`, or `40%` solved / `60%` fallback when facing a bet.

### Pseudo-Harmonic Action Translation (`src/live/protocol.rs`)
Off-tree continuous opponent bet sizes $x \in [A, B]$ between adjacent discrete game-tree sizes $A$ and $B$ map probabilistically to preserve pot-odds expectation (Ganzfried & Sandholm 2013):

$$P(A) = \frac{(B - x)(1 + A)}{(B - A)(1 + x)}, \qquad P(B) = 1 - P(A)$$

---

## 6. Leduc Hold'em Exact Analytical Testbed

Leduc Hold'em (`src/game/leduc.rs`, 0-based rounds `0` and `1`) serves as the exact verification suite alongside 52-card Hold'em:
* `train_vanilla` (`src/cfr/vanilla.rs`) performs full game-tree CFR+ traversal without sampling variance.
* `evaluate --game leduc` (`src/sim/leduc/eval.rs`) computes exact **Best Response** and **NashConv exploitability** alongside large-sample simulations.
