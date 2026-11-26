/*! Opponent Profile Decay, Blind Adaptation, and JSON Persistence */
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use super::OpponentTracker;

const DEFAULT_PROFILES_PATH: &str = "models/opponent_profiles.json";
const TMP_PROFILES_FALLBACK: &str = "/tmp/opponent_profiles.json";

impl OpponentTracker {
    /** Applies exponential session decay to historical counters so recent hands adapt faster */
    pub fn apply_session_decay(&mut self, factor: f64) {
        let f = factor.clamp(0.1, 1.0);
        let scale = |v: &mut u64| *v = (*v as f64 * f).round() as u64;
        scale(&mut self.total_hands);
        scale(&mut self.vpip_hands);
        scale(&mut self.pfr_hands);
        scale(&mut self.fold_count);
        scale(&mut self.call_count);
        scale(&mut self.check_count);
        scale(&mut self.raise_count);
        for a in &mut self.preflop_actions {
            scale(a);
        }
        for a in &mut self.flop_actions {
            scale(a);
        }
        scale(&mut self.flop_checks);
        for a in &mut self.turn_actions {
            scale(a);
        }
        scale(&mut self.turn_checks);
        for a in &mut self.river_actions {
            scale(a);
        }
        scale(&mut self.river_checks);
        scale(&mut self.flop_cbet_faced);
        scale(&mut self.flop_fold_to_cbet);
        scale(&mut self.turn_cbet_faced);
        scale(&mut self.turn_fold_to_cbet);
        scale(&mut self.river_bet_faced);
        scale(&mut self.river_fold_to_bet);
        scale(&mut self.went_to_showdown);
        scale(&mut self.saw_flop);
        scale(&mut self.sizing_small_bets);
        scale(&mut self.sizing_mid_bets);
        scale(&mut self.sizing_large_bets);
        scale(&mut self.postflop_overbets);
        scale(&mut self.river_overbets);
        scale(&mut self.postflop_bets_total);
        scale(&mut self.showdown_small_bluff_count);
        scale(&mut self.showdown_large_monster_count);
        scale(&mut self.showdown_hands);
        scale(&mut self.showdown_bluff_count);
        scale(&mut self.showdown_thin_value_pair_count);
        scale(&mut self.showdown_trap_count);
        scale(&mut self.showdown_loose_count);
        scale(&mut self.flop_cbet_opps);
        scale(&mut self.flop_cbet_made);
        scale(&mut self.turn_barrel_opps);
        scale(&mut self.turn_barrel_made);
        scale(&mut self.showdown_overbet_hands);
        scale(&mut self.showdown_overbet_value_hands);
        scale(&mut self.sb_vpip_hands);
        scale(&mut self.sb_pfr_hands);
        scale(&mut self.sb_total_hands);
        scale(&mut self.bb_vpip_hands);
        scale(&mut self.bb_total_hands);
        scale(&mut self.faced_3bet_count);
        scale(&mut self.fold_to_3bet_count);

        self.vpip_hands = self.vpip_hands.min(self.total_hands);
        self.pfr_hands = self.pfr_hands.min(self.vpip_hands);
        self.check_count = self.check_count.min(self.call_count);
        self.postflop_overbets = self.postflop_overbets.min(self.postflop_bets_total);
        self.river_overbets = self.river_overbets.min(self.postflop_overbets);
        self.sb_vpip_hands = self.sb_vpip_hands.min(self.sb_total_hands);
        self.sb_pfr_hands = self.sb_pfr_hands.min(self.sb_vpip_hands);
        self.bb_vpip_hands = self.bb_vpip_hands.min(self.bb_total_hands);
        self.fold_to_3bet_count = self.fold_to_3bet_count.min(self.faced_3bet_count);
        self.flop_cbet_made = self.flop_cbet_made.min(self.flop_cbet_opps);
        self.turn_barrel_made = self.turn_barrel_made.min(self.turn_barrel_opps);
        self.showdown_overbet_value_hands = self
            .showdown_overbet_value_hands
            .min(self.showdown_overbet_hands);
    }

    /** Decay historical statistics when the table Big Blind steps up significantly (e.g. every 20 hands)
     * so opponent profiling adapts rapidly to high-blind / short-stack strategy shifts. */
    pub fn observe_blind_level(&mut self, current_bb: i32) {
        if current_bb <= 0 {
            return;
        }
        if self.last_observed_bb > 0 && current_bb > self.last_observed_bb {
            let ratio = current_bb as f64 / self.last_observed_bb as f64;
            if ratio >= 1.35 {
                let factor = (1.0 / ratio.sqrt()).clamp(0.45, 0.72);
                self.apply_session_decay(factor);
            }
        }
        self.last_observed_bb = current_bb;
    }

    /** Persist opponent profile to disk JSON database keyed by opponent identifier,
     * with automatic fallback to `/tmp/opponent_profiles.json` on read-only container roots */
    pub fn save_profile(
        &self,
        path: &str,
        opp_key: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if opp_key.is_empty() {
            return Ok(());
        }
        let mut map = read_profile_map(path)?;
        if path == DEFAULT_PROFILES_PATH
            && let Ok(tmp_map) = read_profile_map(TMP_PROFILES_FALLBACK)
        {
            for (k, v) in tmp_map {
                map.insert(k, v);
            }
        }
        map.insert(opp_key.to_string(), self.clone());
        if let Some(parent) = Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            let _ = std::fs::create_dir_all(parent);
        }
        if write_profile_map_atomic(path, &map).is_ok() {
            return Ok(());
        }
        write_profile_map_atomic(TMP_PROFILES_FALLBACK, &map)?;
        Ok(())
    }

    /** Load opponent profile from disk JSON database (checking `/tmp` runtime fallback first)
     * and apply `decay` exponential session decay factor */
    pub fn load_profile_with_decay(path: &str, opp_key: &str, decay: f64) -> Option<Self> {
        if opp_key.is_empty() {
            return None;
        }
        let mut loaded_tracker: Option<OpponentTracker> = if path == DEFAULT_PROFILES_PATH {
            read_profile_map(TMP_PROFILES_FALLBACK)
                .ok()
                .and_then(|mut map| map.remove(opp_key))
        } else {
            None
        };

        if loaded_tracker.is_none() {
            loaded_tracker = read_profile_map(path)
                .ok()
                .and_then(|mut map| map.remove(opp_key));
        }

        let mut tracker = loaded_tracker?;
        tracker.hand_vpip_recorded = false;
        tracker.hand_pfr_recorded = false;
        tracker.hand_saw_flop_recorded = false;
        tracker.hand_pf_raises = 0;
        tracker.hand_post_raises = 0;
        tracker.hand_flop_cbet_recorded = false;
        tracker.hand_flop_bet_made = false;
        tracker.hand_turn_barrel_recorded = false;
        tracker.last_observed_bb = 0;
        tracker.apply_session_decay(decay);
        Some(tracker)
    }

    /** Load opponent profile from disk JSON database (checking `/tmp` runtime fallback first)
     * and apply default `0.90` session decay */
    pub fn load_profile(path: &str, opp_key: &str) -> Option<Self> {
        Self::load_profile_with_decay(path, opp_key, 0.90)
    }
}

fn read_profile_map(
    path: &str,
) -> Result<HashMap<String, OpponentTracker>, Box<dyn std::error::Error>> {
    if !Path::new(path).exists() {
        return Ok(HashMap::new());
    }
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader).unwrap_or_default())
}

fn write_profile_map_atomic(
    path: &str,
    map: &HashMap<String, OpponentTracker>,
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp_path = format!("{path}.tmp");
    let write_res = (|| -> Result<(), Box<dyn std::error::Error>> {
        let file = File::create(&tmp_path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, map)?;
        writer.flush()?;
        drop(writer);
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    })();
    if write_res.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    write_res
}
