//! Markdown role file parser.
//!
//! Parses role Markdown files (like those in `roles/`) into [`RawRoleSource`]
//! intermediate representations. The parser is deliberately lenient: it
//! extracts whatever structure it can find, leaving validation to later stages.

use std::collections::HashMap;

use crate::error::{RoleError, RoleResult};
use crate::model::RawRoleSource;

/// Parses Markdown role files into [`RawRoleSource`] representations.
pub struct RoleParser;

impl RoleParser {
    /// Parse a Markdown string into a [`RawRoleSource`].
    ///
    /// The parser extracts the H1 heading as the title and all H2 sections
    /// as key-value pairs. The section heading is lowercased and used as key;
    /// the body text is preserved verbatim (trimmed).
    pub fn parse(
        content: &str,
        source_path: &str,
        department_dir: Option<&str>,
    ) -> RoleResult<RawRoleSource> {
        let mut raw = RawRoleSource {
            source_path: source_path.to_string(),
            department_dir: department_dir.map(|s| s.to_string()),
            title: None,
            sections: HashMap::new(),
        };

        let mut current_section: Option<String> = None;
        let mut current_body = String::new();

        for line in content.lines() {
            let trimmed = line.trim();

            if let Some(heading) = trimmed.strip_prefix("# ") {
                // H1 heading — role title
                if !heading.trim().is_empty() && raw.title.is_none() {
                    raw.title = Some(heading.trim().to_string());
                }
                continue;
            }

            if let Some(heading) = trimmed.strip_prefix("## ") {
                // H2 heading — new section. Flush the previous section.
                if let Some(ref section_key) = current_section {
                    let body = current_body.trim().to_string();
                    if !body.is_empty() {
                        raw.sections.insert(section_key.clone(), body);
                    }
                }
                current_section = Some(heading.trim().to_lowercase());
                current_body = String::new();
                continue;
            }

            // Accumulate body text for the current section.
            if current_section.is_some() {
                current_body.push_str(line);
                current_body.push('\n');
            }
        }

        // Flush the last section.
        if let Some(ref section_key) = current_section {
            let body = current_body.trim().to_string();
            if !body.is_empty() {
                raw.sections.insert(section_key.clone(), body);
            }
        }

        if raw.title.is_none() && raw.sections.is_empty() {
            return Err(RoleError::ParseFailed {
                path: source_path.to_string(),
                reason: "no title or sections found".to_string(),
            });
        }

        Ok(raw)
    }

    /// Extract a bullet list from a section body.
    ///
    /// Recognizes lines starting with `- ` or `* ` and returns the trimmed
    /// items. Non-list lines are ignored.
    pub fn extract_list(body: &str) -> Vec<String> {
        body.lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("- ") {
                    Some(rest.trim().to_string())
                } else {
                    trimmed
                        .strip_prefix("* ")
                        .map(|rest| rest.trim().to_string())
                }
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Extract a numbered list from a section body.
    ///
    /// Recognizes lines like `1. item` and returns the items without the number prefix.
    pub fn extract_numbered_list(body: &str) -> Vec<String> {
        let re = regex::Regex::new(r"^\d+\.\s+(.+)$").expect("valid regex");
        body.lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                re.captures(trimmed)
                    .and_then(|caps| caps.get(1))
                    .map(|m| m.as_str().trim().to_string())
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Extract the first paragraph (non-list, non-heading text block).
    pub fn extract_paragraph(body: &str) -> String {
        let mut para = String::new();
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() && !para.is_empty() {
                break;
            }
            if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with('#') {
                if !para.is_empty() {
                    break;
                }
                continue;
            }
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(trimmed);
        }
        para.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MD: &str = r#"# CEO Agent

## Profile

The CEO Agent holds the overall company perspective.

## Mission

Set a clear direction and ensure sustainable value.

## Responsibilities

- company strategy
- capital allocation
- executive alignment

## KPIs

- revenue growth
- profitability

## Prompt Template

You are the CEO Agent.
Your job is to evaluate decisions.

Structure:

1. current situation
2. strategic importance
3. recommendation
"#;

    #[test]
    fn parses_basic_role_file() {
        let raw = RoleParser::parse(
            SAMPLE_MD,
            "00_GOVERNANCE/CEO_Agent.md",
            Some("00_GOVERNANCE"),
        )
        .unwrap();
        assert_eq!(raw.title.as_deref(), Some("CEO Agent"));
        assert!(raw.sections.contains_key("profile"));
        assert!(raw.sections.contains_key("mission"));
        assert!(raw.sections.contains_key("responsibilities"));
        assert!(raw.sections.contains_key("kpis"));
        assert!(raw.sections.contains_key("prompt template"));
    }

    #[test]
    fn extract_list_from_section() {
        let body = "- company strategy\n- capital allocation\n- executive alignment";
        let items = RoleParser::extract_list(body);
        assert_eq!(
            items,
            vec![
                "company strategy",
                "capital allocation",
                "executive alignment"
            ]
        );
    }

    #[test]
    fn extract_numbered_list() {
        let body = "1. current situation\n2. strategic importance\n3. recommendation";
        let items = RoleParser::extract_numbered_list(body);
        assert_eq!(
            items,
            vec![
                "current situation",
                "strategic importance",
                "recommendation"
            ]
        );
    }

    #[test]
    fn extract_paragraph() {
        let body = "The CEO Agent holds the overall company perspective.\nIt ensures alignment.";
        let para = RoleParser::extract_paragraph(body);
        assert_eq!(
            para,
            "The CEO Agent holds the overall company perspective. It ensures alignment."
        );
    }

    #[test]
    fn rejects_empty_file() {
        let result = RoleParser::parse("", "empty.md", None);
        assert!(result.is_err());
    }
}
