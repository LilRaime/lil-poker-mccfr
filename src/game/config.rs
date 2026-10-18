/*
 *        Texas Hold'em Game Configuration
 * Change values here to retune the bot for any game size
 *
 * All chip values are in integer chips (not big-blind units).
 * After changing these, you must retrain the blueprint model.
 */

/* ── Stack & Blind Structure ──────────────────────────────── */

pub const STACK_SIZE: i32 = 10_000;
pub const SMALL_BLIND: i32 = 50;
pub const BIG_BLIND: i32 = 100;
pub const BIG_BLIND_F64: f64 = BIG_BLIND as f64;

/* ── Game Architecture & Dimensions ─────────────────────── */

pub const NUM_PLAYERS: usize = 2;
pub const NUM_ROUNDS: usize = 4;
pub const NUM_CARDS_DECK: usize = 52;
pub const NUM_HOLE_CARDS: usize = 2;
pub const NUM_COMMUNITY_CARDS: usize = 5;
pub const NUM_SUITS: usize = 4;
pub const NUM_RANKS: usize = 13;
pub const MAX_ROUND_ACTIONS: usize = 32;

/// Number of postflop equity abstraction buckets (0..=76).
pub const NUM_EQUITY_BUCKETS: usize = 77;

/// Number of canonical preflop starting hand combinations (13 pairs + 78 suited + 78 offsuited).
pub const NUM_PREFLOP_BUCKETS: usize = 169;

/* ── Action Constants ─────────────────────────────────────── */

pub const FOLD: u8 = 0;
pub const CALL_CHECK: u8 = 1;
pub const RAISE_MIN: u8 = 2;
pub const RAISE_THIRD_POT: u8 = 3;
pub const RAISE_HALF_POT: u8 = 4;
pub const ALL_IN: u8 = 5;
pub const HOLDEM_NUM_ACTIONS: usize = 6;

/* ── Solver & Runtime Defaults ───────────────────────────── */

pub const DEFAULT_SUBGAME_ITERATIONS: usize = 2500;

/* ── Raise Sizing Floors ──────────────────────────────────── */

pub const RAISE_MIN_AMT: i32 = BIG_BLIND * 4;
pub const PREFLOP_OPEN_MIN_TOTAL: i32 = BIG_BLIND * 5 / 2;
pub const RAISE_THIRD_POT_FLOOR: i32 = BIG_BLIND * 3;
pub const RAISE_HALF_POT_FLOOR_DRY: i32 = BIG_BLIND * 4;
pub const RAISE_HALF_POT_FLOOR_WET: i32 = BIG_BLIND * 5;
pub const RAISE_HALF_POT_FLOOR_TURN: i32 = BIG_BLIND * 6;
pub const RAISE_HALF_POT_FLOOR_RIVER: i32 = BIG_BLIND * 8;
pub const RAISE_OVERBET_FLOOR: i32 = BIG_BLIND * 10;

/* ── Betting Round Limits ─────────────────────────────────── */

pub const MAX_RAISES_PER_ROUND: u8 = 3;

/* ── Subgame Solver Thresholds ────────────────────────────── */

pub const SUBGAME_POT_LG: i32 = STACK_SIZE * 8 / 10; // 80% of stack
pub const SUBGAME_POT_MD: i32 = STACK_SIZE * 4 / 10; // 40% of stack
pub const SUBGAME_POT_SM: i32 = STACK_SIZE * 2 / 10; // 20% of stack
pub const SUBGAME_POT_XS: i32 = BIG_BLIND * 6; // 6 BB (small pot)

/// To-call amount above which subgame solver gets extra budget (≈ 10× BB).
pub const SUBGAME_BET_PRESSURE: i32 = BIG_BLIND * 10;

/// Pot threshold to activate flop subgame search (≈ 12× BB).
pub const SUBGAME_FLOP_TRIGGER_POT: i32 = BIG_BLIND * 12;

/* ── Action Recognition Thresholds (play_live) ───────────── */

/// Bet at/below this is classified as "limp" preflop (= BB + ½ BB).
pub const LIMP_THRESHOLD: i32 = BIG_BLIND * 2;

/// Bet at/above this classified as ALL_IN signal from server.
pub const ALLIN_BET_THRESHOLD: i32 = STACK_SIZE * 8 / 10;

/// Bet at/above this classified as RAISE_HALF_POT action (≈ 6× BB).
pub const HALF_POT_RAISE_THRESHOLD: i32 = BIG_BLIND * 6;

/// Bet at/above this classified as RAISE_THIRD_POT action (≈ 4× BB).
pub const THIRD_POT_RAISE_THRESHOLD: i32 = BIG_BLIND * 4;

/* ── Opponent Model & Purification Thresholds ────────────── */

/// to_call above this triggers all-in defense (heavy bet = 35× BB).
pub const ALLIN_GUARD_THRESHOLD: i32 = BIG_BLIND * 35;

/// Preflop to_call at/below which Button steal redistribution fires (= 1× BB).
pub const PREFLOP_STEAL_MAX_TO_CALL: i32 = BIG_BLIND;

/* ── Fallback Strategy to_call Tiers (preflop) ───────────── */

/// Tier-1 (Premium) hands: call/3-bet up to this amount.
pub const PF_TIER1_CALL_MAX: i32 = BIG_BLIND * 8;

/// Tier-2 (Strong) hands: defend up to this amount.
pub const PF_TIER2_CALL_MAX: i32 = BIG_BLIND * 4;

/// Tier-3 (Speculative) hands: cheap-call up to this amount.
pub const PF_TIER3_CHEAP_CALL_MAX: i32 = BIG_BLIND * 2;

/// Tier-3 (Speculative) hands: reluctant call/fold boundary.
pub const PF_TIER3_FOLD_BOUNDARY: i32 = BIG_BLIND * 6;

/// Tier-4 (Marginal) hands: minimum call threshold.
pub const PF_TIER4_CALL_MAX: i32 = BIG_BLIND * 2;
