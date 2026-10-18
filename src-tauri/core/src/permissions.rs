use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::models::{Mode, PermissionMode};

/// A user-authored allow/deny rule.
///
/// `pattern` is matched against the tool name alone (`bash`) or against
/// `tool:argument` (`bash:npm test`, `write:src/**`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    /// `allow` grants, `deny` refuses. Deny always wins over allow.
    pub effect: Effect,
    /// What the rule applies to: a tool name or `tool:pattern`.
    pub pattern: String,
    /// When true the agent must still ask; the rule only pre-approves it in
    /// Auto-review mode.
    pub ask_first: bool,
    pub project_id: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Deny,
}

/// Outcome of checking one tool call against the rule set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    /// Run it.
    Allow,
    /// Show the approval prompt and wait for the user.
    Ask,
    /// Refuse; the model is told why so it can pick another approach.
    Deny,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }
}

/// The permission set in force for one session turn.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    pub mode: PermissionMode,
    pub agent_mode: Mode,
    pub rules: Vec<Rule>,
}

impl Policy {
    /// Decides whether one call may run.
    ///
    /// Order matters: an explicit deny is final, then the session mode sets the
    /// baseline, then rules can only tighten it (ask) or, in Full access,
    /// loosen an `ask` baseline. Plan mode can never be widened by a rule.
    pub fn evaluate(&self, tool: &str, argument: Option<&str>) -> Decision {
        let mut allow_match: Option<bool> = None;
        let mut matched_ask = false;

        for r in self.rules.iter().filter(|r| r.enabled) {
            if !rule_matches(r, tool, argument) {
                continue;
            }
            match r.effect {
                Effect::Deny => return Decision::Deny,
                Effect::Allow => {
                    if r.ask_first {
                        matched_ask = true;
                    } else {
                        allow_match = Some(true);
                    }
                }
            }
        }

        // Plan mode is read-only. This is structural, not configurable.
        if self.agent_mode == Mode::Plan && mutating_tool(tool) {
            return Decision::Deny;
        }

        match self.mode {
            PermissionMode::FullAccess => {
                if matched_ask {
                    Decision::Ask
                } else {
                    Decision::Allow
                }
            }
            PermissionMode::AutoReview => {
                // Auto-review does not widen the sandbox: it changes *who*
                // reviews, so the same calls stay runnable and the reviewer (not
                // the user) signs off on mutations.
                if matched_ask { Decision::Ask } else { Decision::Allow }
            }
            PermissionMode::Ask => {
                if allow_match == Some(true) { Decision::Allow } else { Decision::Ask }
            }
        }
    }

    /// Tools that change state. Kept in sync with the registry by a unit test.
    pub fn is_mutating(tool: &str) -> bool {
        mutating_tool(tool)
    }
}

/// Tools that write to disk, run commands or otherwise change the machine.
pub fn mutating_tool(tool: &str) -> bool {
    matches!(
        tool,
        "write" | "edit" | "multi_edit" | "delete" | "move" | "bash" | "patch" | "notebook_edit"
    )
}

fn rule_matches(r: &Rule, tool: &str, argument: Option<&str>) -> bool {
    let subject = match argument {
        Some(a) if !a.is_empty() => format!("{tool}:{a}"),
        _ => tool.to_string(),
    };
    if glob_match(&r.pattern, &subject) {
        return true;
    }
    // A bare tool name matches every call to that tool.
    !r.pattern.contains(':') && glob_match(&r.pattern, tool)
}

/// Minimal glob matcher supporting `*`, `**` and `?`.
///
/// Implemented here rather than pulled from `globset` because rules are matched
/// against short strings and must behave identically on every platform, where
/// `globset` normalizes Windows separators.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.replace('\\', "/").chars().collect();
    let t: Vec<char> = text.replace('\\', "/").chars().collect();
    glob_here(&p, 0, &t, 0)
}

fn glob_here(p: &[char], mut pi: usize, t: &[char], mut ti: usize) -> bool {
    while pi < p.len() {
        match p[pi] {
            '*' => {
                // `**` also spans path separators; a single `*` does not.
                let double = p.get(pi + 1) == Some(&'*');
                let mut next = pi + if double { 2 } else { 1 };
                if double && p.get(next) == Some(&'/') {
                    next += 1;
                }
                // A trailing `**` consumes whatever is left, separators included.
                if double && next >= p.len() {
                    return true;
                }
                let mut k = ti;
                loop {
                    if glob_here(p, next, t, k) {
                        return true;
                    }
                    if k >= t.len() {
                        return false;
                    }
                    if !double && t[k] == '/' {
                        return false;
                    }
                    k += 1;
                }
            }
            '?' => {
                if ti >= t.len() || t[ti] == '/' {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
            c => {
                if ti >= t.len() || t[ti] != c {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
        }
    }
    ti == t.len()
}

/// Feeds a refusal back to the model so it can adapt rather than retry blindly.
pub fn denial_message(tool: &str, reason: &str) -> String {
    format!(
        "The user refused the `{tool}` call{}. \
         Do not retry the same call. Either ask what they would prefer, or take a \
         different approach that does not need this action.",
        if reason.is_empty() { String::new() } else { format!(": {reason}") }
    )
}

pub fn require_mode_error(tool: &str) -> Error {
    Error::Denied(format!("`{tool}` is not available while the session is in Plan mode"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(effect: Effect, pattern: &str, ask_first: bool) -> Rule {
        Rule {
            id: "r".into(),
            effect,
            pattern: pattern.into(),
            ask_first,
            project_id: None,
            enabled: true,
        }
    }

    #[test]
    fn ask_mode_prompts_by_default() {
        let p = Policy { mode: PermissionMode::Ask, ..Default::default() };
        assert_eq!(p.evaluate("read", Some("a.rs")), Decision::Ask);
    }

    #[test]
    fn allow_rule_short_circuits_ask_mode() {
        let p = Policy {
            mode: PermissionMode::Ask,
            rules: vec![rule(Effect::Allow, "read", false)],
            ..Default::default()
        };
        assert_eq!(p.evaluate("read", Some("a.rs")), Decision::Allow);
        assert_eq!(p.evaluate("write", Some("a.rs")), Decision::Ask);
    }

    #[test]
    fn deny_always_wins_even_in_full_access() {
        let p = Policy {
            mode: PermissionMode::FullAccess,
            rules: vec![rule(Effect::Deny, "bash", false)],
            ..Default::default()
        };
        assert_eq!(p.evaluate("bash", Some("rm -rf /")), Decision::Deny);
    }

    #[test]
    fn plan_mode_refuses_mutations_but_allows_reads() {
        let p = Policy {
            mode: PermissionMode::FullAccess,
            agent_mode: Mode::Plan,
            ..Default::default()
        };
        assert_eq!(p.evaluate("read", Some("a.rs")), Decision::Allow);
        assert_eq!(p.evaluate("write", Some("a.rs")), Decision::Deny);
        assert_eq!(p.evaluate("bash", Some("ls")), Decision::Deny);
    }

    #[test]
    fn a_plan_rule_cannot_widen_plan_mode() {
        let p = Policy {
            mode: PermissionMode::FullAccess,
            agent_mode: Mode::Plan,
            rules: vec![rule(Effect::Allow, "write:src/**", false)],
            ..Default::default()
        };
        assert_eq!(p.evaluate("write", Some("src/a.rs")), Decision::Deny);
    }

    #[test]
    fn ask_first_rule_escalates_even_under_full_access() {
        let p = Policy {
            mode: PermissionMode::FullAccess,
            rules: vec![rule(Effect::Allow, "bash", true)],
            ..Default::default()
        };
        assert_eq!(p.evaluate("bash", Some("npm test")), Decision::Ask);
    }

    #[test]
    fn rules_match_on_tool_and_argument() {
        let r = rule(Effect::Allow, "bash:npm test", false);
        assert!(rule_matches(&r, "bash", Some("npm test")));
        assert!(!rule_matches(&r, "bash", Some("npm run build")));
        // An argument-scoped rule cannot match a call that has no argument.
        assert!(!rule_matches(&r, "bash", None));
    }

    #[test]
    fn a_bare_tool_name_matches_every_call() {
        let r = rule(Effect::Allow, "read", false);
        assert!(rule_matches(&r, "read", Some("a.rs")));
        assert!(rule_matches(&r, "read", Some("b/c.rs")));
        assert!(rule_matches(&r, "read", None));
        assert!(!rule_matches(&r, "write", None));
    }

    #[test]
    fn disabled_rules_are_ignored() {
        let mut r = rule(Effect::Deny, "bash", false);
        r.enabled = false;
        let p = Policy {
            mode: PermissionMode::FullAccess,
            rules: vec![r],
            ..Default::default()
        };
        assert_eq!(p.evaluate("bash", Some("ls")), Decision::Allow);
    }

    #[test]
    fn glob_star_does_not_cross_separators() {
        assert!(glob_match("src/*.rs", "src/a.rs"));
        assert!(!glob_match("src/*.rs", "src/deep/a.rs"));
    }

    #[test]
    fn glob_double_star_crosses_separators() {
        assert!(glob_match("src/**/*.rs", "src/a/b/c.rs"));
        assert!(glob_match("**", "anything/at/all"));
        assert!(glob_match("*.rs", "a.rs"));
        assert!(!glob_match("*.rs", "dir/a.rs"));
    }

    #[test]
    fn glob_handles_windows_separators() {
        assert!(glob_match("src/**", "src\\a\\b.rs"));
    }

    #[test]
    fn glob_question_mark_matches_one_char() {
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(!glob_match("a?c", "a/c"));
    }

    #[test]
    fn denial_message_names_the_tool() {
        let m = denial_message("write", "outside the workspace");
        assert!(m.contains("write"));
        assert!(m.contains("outside the workspace"));
        assert!(m.contains("Do not retry"));
    }
}