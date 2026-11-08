//! Custom Heuristic Rules Engine for C.U.R.E.
//!
//! Enables security engineers and developers to drop `.rule` files into
//! `%APPDATA%\cure\rules` with custom string/heuristic patterns to catch
//! emerging threats, red-team tools, or custom enterprise indicators.

use crate::model::RiskLevel;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomRule {
    pub name: String,
    pub pattern: String,
    pub risk: RiskLevel,
    pub description: String,
}

/// Parse `.rule` file contents into a list of [`CustomRule`]s.
pub fn parse_rules(content: &str) -> Vec<CustomRule> {
    let mut rules = Vec::new();
    let mut current_name = String::new();
    let mut current_pattern = String::new();
    let mut current_risk = RiskLevel::Suspicious;
    let mut current_desc = String::new();

    let flush_rule = |rules: &mut Vec<CustomRule>,
                      name: &mut String,
                      pattern: &mut String,
                      risk: &mut RiskLevel,
                      desc: &mut String| {
        if !name.trim().is_empty() && !pattern.trim().is_empty() {
            rules.push(CustomRule {
                name: name.trim().to_string(),
                pattern: pattern.trim().to_string(),
                risk: *risk,
                description: if desc.trim().is_empty() {
                    format!("Custom rule pattern: {}", pattern.trim())
                } else {
                    desc.trim().to_string()
                },
            });
        }
        name.clear();
        pattern.clear();
        *risk = RiskLevel::Suspicious;
        desc.clear();
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with("//")
            || line.starts_with(';')
        {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            flush_rule(
                &mut rules,
                &mut current_name,
                &mut current_pattern,
                &mut current_risk,
                &mut current_desc,
            );
            let inside = &line[1..line.len() - 1].trim();
            let clean_name = if let Some(stripped) = inside.strip_prefix("Rule:") {
                stripped.trim()
            } else if let Some(stripped) = inside.strip_prefix("rule:") {
                stripped.trim()
            } else {
                inside
            };
            current_name = clean_name.to_string();
            continue;
        }

        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            let val = val.trim();
            match key.as_str() {
                "pattern" => current_pattern = val.to_string(),
                "risk" => {
                    if val.eq_ignore_ascii_case("highrisk") || val.eq_ignore_ascii_case("high") {
                        current_risk = RiskLevel::HighRisk;
                    } else if val.eq_ignore_ascii_case("safe") {
                        current_risk = RiskLevel::Safe;
                    } else {
                        current_risk = RiskLevel::Suspicious;
                    }
                }
                "description" | "desc" => current_desc = val.to_string(),
                "name" if current_name.is_empty() => {
                    current_name = val.to_string();
                }
                _ => {}
            }
        }
    }

    flush_rule(
        &mut rules,
        &mut current_name,
        &mut current_pattern,
        &mut current_risk,
        &mut current_desc,
    );

    rules
}

/// Load all `.rule` files from the given directory (e.g. `%APPDATA%\cure\rules`).
/// Returns an empty Vec if the directory does not exist or has no valid rules.
pub fn load_rules_from_dir(dir: &Path) -> Vec<CustomRule> {
    let mut all_rules = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return all_rules;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("rule") {
            if let Ok(content) = fs::read_to_string(&path) {
                let parsed = parse_rules(&content);
                all_rules.extend(parsed);
            }
        }
    }

    all_rules
}

/// Check if the candidate text (e.g. command line, process name, or file path)
/// triggers any loaded custom rule (case-insensitive substring match).
pub fn evaluate_custom_rules<'a>(
    haystack: &str,
    rules: &'a [CustomRule],
) -> Option<&'a CustomRule> {
    if haystack.is_empty() || rules.is_empty() {
        return None;
    }
    let hay_lower = haystack.to_ascii_lowercase();
    for rule in rules {
        let pat_lower = rule.pattern.to_ascii_lowercase();
        if hay_lower.contains(&pat_lower) {
            return Some(rule);
        }
    }
    None
}

/// Default template content written when the rules directory is created.
pub const SAMPLE_RULE_TEMPLATE: &str = r#"# ==============================================================================
# C.U.R.E Custom Threat Heuristic Rules
# Drop any .rule files into this folder (%APPDATA%\cure\rules) to add custom signatures.
# ==============================================================================

[Rule: Reverse Shell Netcat]
pattern = nc.exe -e cmd.exe
risk = HighRisk
description = Remote cmd reverse shell invocation via netcat

[Rule: PowerShell Base64 Encoded Command]
pattern = -EncodedCommand
risk = HighRisk
description = Obfuscated PowerShell script execution

[Rule: Suspicious Temp Download Tool]
pattern = curl -s -o %TEMP%
risk = Suspicious
description = Remote payload staging in temporary folder
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_rule() {
        let text = r#"
        [Rule: Test Drop]
        pattern = evil_payload.exe
        risk = HighRisk
        description = Malicious executable test
        "#;
        let rules = parse_rules(text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "Test Drop");
        assert_eq!(rules[0].pattern, "evil_payload.exe");
        assert_eq!(rules[0].risk, RiskLevel::HighRisk);
        assert_eq!(rules[0].description, "Malicious executable test");
    }

    #[test]
    fn parse_multiple_rules_and_evaluate() {
        let rules = parse_rules(SAMPLE_RULE_TEMPLATE);
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].name, "Reverse Shell Netcat");
        assert_eq!(rules[1].name, "PowerShell Base64 Encoded Command");

        // Test evaluation
        let matched =
            evaluate_custom_rules("powershell.exe -NoProfile -encodedcommand aGVsbG8=", &rules);
        assert!(matched.is_some());
        let m = matched.unwrap();
        assert_eq!(m.name, "PowerShell Base64 Encoded Command");
        assert_eq!(m.risk, RiskLevel::HighRisk);

        let unmatched = evaluate_custom_rules("notepad.exe C:\\notes.txt", &rules);
        assert!(unmatched.is_none());
    }
}
