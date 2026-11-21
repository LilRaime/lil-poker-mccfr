/*! Live HTTP Rejected-Action and Emergency Fallback Candidate Selection */

/** Input parameters for building safe emergency fallback action candidates */
#[derive(Debug, Clone, Copy, Default)]
pub struct EmergencyFallbackRequest<'a> {
    pub legal_actions: &'a [String],
    pub chosen_action: &'a str,
    pub chosen_amount: i32,
    pub first_retry: Option<(&'a str, i32)>,
    pub err_text: &'a str,
    pub to_call: i32,
    pub my_chips: i32,
}

fn error_demands_allin(err_text: &str) -> bool {
    let lower = err_text.to_ascii_lowercase();
    lower.contains("all_in")
        || lower.contains("allin")
        || lower.contains("all-in")
        || lower.contains("all in")
}

fn has_legal_tok(legal_actions: &[String], tok: &str) -> bool {
    legal_actions.iter().any(|a| a.eq_ignore_ascii_case(tok))
}

pub(super) fn choose_rejected_call_fallback(
    legal_actions: &[String],
    err_text: &str,
    to_call: i32,
    my_chips: i32,
) -> Option<(String, i32)> {
    let call_is_allin = my_chips > 0 && to_call > 0 && my_chips <= to_call;
    let has_allin_only = !has_legal_tok(legal_actions, "call")
        && !has_legal_tok(legal_actions, "check")
        && (has_legal_tok(legal_actions, "all_in") || has_legal_tok(legal_actions, "allin"));
    let err_allin = error_demands_allin(err_text);

    if to_call == 0 && has_legal_tok(legal_actions, "check") {
        Some(("check".to_string(), 0))
    } else if to_call > 0 && has_legal_tok(legal_actions, "call") && !err_allin {
        Some(("call".to_string(), to_call))
    } else if (call_is_allin || has_allin_only || err_allin)
        && has_legal_tok(legal_actions, "all_in")
    {
        Some(("all_in".to_string(), 0))
    } else if (call_is_allin || has_allin_only || err_allin)
        && has_legal_tok(legal_actions, "allin")
    {
        Some(("allin".to_string(), 0))
    } else if call_is_allin || err_allin {
        Some(("all_in".to_string(), 0))
    } else if has_legal_tok(legal_actions, "check") {
        Some(("check".to_string(), 0))
    } else {
        None
    }
}

fn append_allin_candidates(legal_actions: &[String], raw: &mut Vec<(&'static str, i32)>) {
    if has_legal_tok(legal_actions, "all_in") {
        raw.push(("all_in", 0));
        if has_legal_tok(legal_actions, "allin") {
            raw.push(("allin", 0));
        }
    } else if has_legal_tok(legal_actions, "allin") {
        raw.push(("allin", 0));
    } else {
        raw.push(("all_in", 0));
        raw.push(("allin", 0));
    }
}

/** Build ordered non-escalating emergency fallback candidates, skipping already attempted actions */
pub fn build_emergency_fallback_candidates(
    req: &EmergencyFallbackRequest<'_>,
) -> Vec<(&'static str, i32)> {
    let legal_empty = req.legal_actions.is_empty();
    let call_is_allin = req.my_chips > 0 && req.to_call > 0 && req.my_chips <= req.to_call;
    let has_allin_only = !has_legal_tok(req.legal_actions, "call")
        && !has_legal_tok(req.legal_actions, "check")
        && (has_legal_tok(req.legal_actions, "all_in")
            || has_legal_tok(req.legal_actions, "allin"));
    let allow_allin = call_is_allin || has_allin_only || error_demands_allin(req.err_text);

    let mut raw: Vec<(&'static str, i32)> = Vec::with_capacity(5);
    if req.to_call > 0 && (has_legal_tok(req.legal_actions, "call") || legal_empty) {
        raw.push(("call", req.to_call));
        raw.push(("call", 0));
    }
    if req.to_call == 0 || has_legal_tok(req.legal_actions, "check") {
        raw.push(("check", 0));
    }
    if allow_allin {
        append_allin_candidates(req.legal_actions, &mut raw);
    }

    let mut out: Vec<(&'static str, i32)> = Vec::with_capacity(raw.len());
    for cand in raw {
        let same_chosen =
            cand.0.eq_ignore_ascii_case(req.chosen_action) && cand.1 == req.chosen_amount;
        let same_retry = req
            .first_retry
            .is_some_and(|(ra, ramt)| cand.0.eq_ignore_ascii_case(ra) && cand.1 == ramt);
        if !same_chosen && !same_retry && !out.contains(&cand) {
            out.push(cand);
        }
    }
    out
}
