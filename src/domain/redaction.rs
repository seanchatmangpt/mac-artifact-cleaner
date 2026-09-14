//! Privacy and redaction helper rules.
//!
//! Provides functions to identify and sanitize sensitive local information (e.g. user home paths and credentials)
//! before reports or plans are output for sharing or version control.

use serde::{Deserialize, Serialize};

/// Credential-shaped keys `redact_content`/`find_credential_matches` treat as
/// sensitive when found followed by a separator (`:`, `=`, `is`, whitespace)
/// and a value. Single source of truth for this list — both the mutating
/// redactor and the non-mutating ledger builder read from here.
const CREDENTIAL_KEYS: [&str; 10] = [
    "password",
    "secret",
    "token",
    "api_key",
    "apikey",
    "credential",
    "passwd",
    "aws_access_key",
    "aws_secret_key",
    "client_secret",
];

/// Ledger of what was redacted from a serialized document, keyed by a stable
/// label, valued by the category matched (e.g. `"path"`, `"credential:password"`).
/// Never stores the original sensitive value — only that a category of
/// sensitive content was found and removed, for audit/logging purposes.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct RedactionLedger {
    pub entries: Vec<RedactionEntry>,
}

/// One redaction ledger entry: what was found (`label`) and what kind of
/// sensitive content it was (`category`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RedactionEntry {
    pub label: String,
    pub category: String,
}

impl RedactionLedger {
    /// Records one redacted item.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::redaction::RedactionLedger;
    ///
    /// let mut ledger = RedactionLedger::default();
    /// assert!(ledger.is_empty());
    /// ledger.record("/Users/john/...", "path");
    /// assert!(!ledger.is_empty());
    /// assert_eq!(ledger.entries[0].category, "path");
    /// ```
    pub fn record(&mut self, label: &str, category: &str) {
        self.entries
            .push(RedactionEntry { label: label.to_string(), category: category.to_string() });
    }

    /// True when nothing was redacted.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::redaction::RedactionLedger;
    ///
    /// // Positive: a fresh ledger is empty.
    /// let ledger = RedactionLedger::default();
    /// assert!(ledger.is_empty());
    ///
    /// // Negative: a ledger with a recorded entry is not empty.
    /// let mut recorded = RedactionLedger::default();
    /// recorded.record("/Users/example/...", "path");
    /// assert!(!recorded.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Scans `content` (without mutating it) for `/Users/<user>` occurrences
/// and returns the distinct usernames found, in first-seen order. Excludes
/// paths already redacted (`/Users/<user>`).
///
/// Shared by `redact_content` (which uses this to drive its replacements)
/// and `redact_serialized` (which uses this to populate the ledger) so the
/// path-matching pattern lives in exactly one place.
fn find_path_matches(content: &str) -> Vec<String> {
    let mut usernames = Vec::new();
    let mut search_idx = 0;
    while let Some(start_idx) = content[search_idx..].find("/Users/") {
        let abs_start = search_idx + start_idx;
        let after_users = &content[abs_start + 7..];
        let end_idx = after_users
            .find(|c: char| {
                c == '/'
                    || c == '"'
                    || c == '\''
                    || c == '\\'
                    || c.is_whitespace()
                    || c == ','
                    || c == '}'
            })
            .unwrap_or(after_users.len());
        let username = &after_users[..end_idx];
        if !username.is_empty()
            && username != "<user>"
            && !usernames.iter().any(|u: &String| u == username)
        {
            usernames.push(username.to_string());
        }
        search_idx = abs_start + 7;
        if search_idx >= content.len() {
            break;
        }
    }
    usernames
}

/// One detected credential-shaped value: the byte range `[start, end)` of the
/// value in `content` (including surrounding quotes when quoted), whether it
/// was quoted, and which key it was found under.
struct CredentialMatch {
    start: usize,
    end: usize,
    quoted: bool,
    key: &'static str,
}

impl CredentialMatch {
    /// The literal replacement text for this match (quoted or bare).
    fn replacement(&self) -> &'static str {
        if self.quoted {
            "\"[REDACTED]\""
        } else {
            "[REDACTED]"
        }
    }
}

/// Scans `content` (without mutating it) for `CREDENTIAL_KEYS` followed by a
/// separator and a redactable value, returning the matched value spans.
/// Mirrors the detection half of `redact_content`'s credential loop, minus
/// the in-place mutation — shared by `redact_content` (which applies the
/// replacement) and `redact_serialized` (which uses it for the ledger) so
/// the credential-matching pattern lives in exactly one place.
fn find_credential_matches(content: &str) -> Vec<CredentialMatch> {
    let mut matches = Vec::new();
    // Byte ranges already claimed by an earlier-processed key's matched
    // value (e.g. "password: supersecret" claims "supersecret"). Keys are
    // processed in `CREDENTIAL_KEYS` order, mirroring the original mutating
    // implementation, where redacting "supersecret" for `password` removed
    // the substring before the `secret` key's own scan ever ran, so `secret`
    // never spuriously matched inside it. A non-mutating scan must
    // replicate that by refusing to start a later key's match inside an
    // already-claimed range.
    let mut consumed: Vec<(usize, usize)> = Vec::new();
    let lower = content.to_lowercase();

    for key in CREDENTIAL_KEYS.iter() {
        let mut key_idx = 0;
        while let Some(found_idx) = lower[key_idx..].find(key) {
            let abs_found = key_idx + found_idx;

            if consumed.iter().any(|&(s, e)| abs_found >= s && abs_found < e) {
                key_idx = abs_found + key.len();
                if key_idx >= content.len() {
                    break;
                }
                continue;
            }

            let after_key = &content[abs_found + key.len()..];

            // Consume a JSON key's closing quote, if present, once — e.g.
            // `"password":"..."` — before the generic separator scan below,
            // which does not otherwise consume `"` (it must not consume the
            // value's own opening quote).
            let mut temp_idx = if after_key.starts_with('"') { 1 } else { 0 };
            loop {
                let remaining = &after_key[temp_idx..];
                if remaining.is_empty() {
                    break;
                }
                if remaining.starts_with(':')
                    || remaining.starts_with('=')
                    || remaining.starts_with(|c: char| c.is_whitespace())
                {
                    temp_idx += 1;
                } else if remaining.to_lowercase().starts_with("is") {
                    let after_is = &remaining[2..];
                    if after_is.is_empty()
                        || after_is.starts_with(|c: char| c.is_whitespace() || c == ':' || c == '=')
                    {
                        temp_idx += 2;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            let sep_len = temp_idx;

            if sep_len == 0 {
                key_idx = abs_found + key.len();
                if key_idx >= content.len() {
                    break;
                }
                continue;
            }

            let val_start = abs_found + key.len() + sep_len;
            if val_start >= content.len() {
                key_idx = abs_found + key.len();
                continue;
            }

            let val_str = &content[val_start..];
            let mut val_len = 0;
            let mut quotes_char = None;
            let mut chars_val = val_str.chars().peekable();

            if let Some(&q) = chars_val.peek() {
                if q == '"' || q == '\'' {
                    quotes_char = Some(q);
                    chars_val.next();
                    val_len += q.len_utf8();
                }
            }

            if let Some(q) = quotes_char {
                while let Some(&c) = chars_val.peek() {
                    val_len += c.len_utf8();
                    chars_val.next();
                    if c == q {
                        break;
                    }
                }
            } else {
                while let Some(&c) = chars_val.peek() {
                    if c.is_whitespace()
                        || c == ','
                        || c == '}'
                        || c == ']'
                        || c == '\n'
                        || c == '\r'
                    {
                        break;
                    }
                    val_len += c.len_utf8();
                    chars_val.next();
                }
            }

            if val_len == 0 {
                key_idx = abs_found + key.len();
                if key_idx >= content.len() {
                    break;
                }
                continue;
            }

            let extracted_val = &content[val_start..val_start + val_len];
            if extracted_val.contains("[REDACTED") {
                key_idx = abs_found + key.len();
                continue;
            }
            let trimmed_val =
                extracted_val.trim().trim_matches('"').trim_matches('\'').to_lowercase();
            let is_another_key = CREDENTIAL_KEYS.iter().any(|k| {
                trimmed_val == *k
                    || trimmed_val.starts_with(&format!("{}:", k))
                    || trimmed_val.starts_with(&format!("{}=", k))
                    || (trimmed_val.ends_with(':')
                        && CREDENTIAL_KEYS.contains(&trimmed_val.trim_end_matches(':')))
            });
            if is_another_key {
                key_idx = abs_found + key.len();
                continue;
            }

            consumed.push((abs_found, val_start + val_len));
            matches.push(CredentialMatch {
                start: val_start,
                end: val_start + val_len,
                quoted: quotes_char.is_some(),
                key,
            });
            key_idx = val_start + val_len;
            if key_idx >= content.len() {
                break;
            }
        }
    }

    matches
}

/// Redacts an entire serialized document (as produced by `serde_json::to_string`
/// on a receipt/plan/audit struct, or any other text payload) and returns
/// `(redacted_content, ledger)`. Built on top of `redact_content` (calls it
/// internally for the actual replacement) plus `find_path_matches`/
/// `find_credential_matches` (re-run against the original content, before any
/// mutation, to populate the ledger) — no matching logic is duplicated.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::redaction::redact_serialized;
///
/// // Positive: a path and a credential are both redacted and ledgered.
/// let raw = r#"{"path":"/Users/user/dev","password":"hunter2"}"#;
/// let (redacted, ledger) = redact_serialized(raw);
/// assert!(redacted.contains("/Users/<user>/dev"));
/// assert!(redacted.contains("[REDACTED]"));
/// assert_eq!(ledger.entries.len(), 2);
///
/// // Negative: clean content is unchanged with an empty ledger.
/// let (clean, empty_ledger) = redact_serialized("hello world");
/// assert_eq!(clean, "hello world");
/// assert!(empty_ledger.is_empty());
/// ```
pub fn redact_serialized(content: &str) -> (String, RedactionLedger) {
    let redacted = redact_content(content);

    let mut ledger = RedactionLedger::default();
    for username in find_path_matches(content) {
        ledger.record(&format!("/Users/{}", username), "path");
    }
    for m in find_credential_matches(content) {
        ledger.record(&content[m.start..m.end], &format!("credential:{}", m.key));
    }

    (redacted, ledger)
}

/// Redacts a path to hide local user profiles in examples.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::redaction::redact_path;
///
/// // Positive case: user path is redacted
/// assert_eq!(redact_path("/Users/user/dev/project"), "/Users/<user>/dev/project");
///
/// // Negative case: non-user paths are not modified
/// assert_eq!(redact_path("/System/Library"), "/System/Library");
/// ```
pub fn redact_path(path: &str) -> String {
    let mut result = path.to_string();
    if let Some(start_idx) = result.find("/Users/") {
        let after_users = &result[start_idx + 7..];
        let end_idx = after_users.find('/').unwrap_or(after_users.len());
        let username = &after_users[..end_idx];
        if !username.is_empty() && username != "<user>" {
            let target = format!("/Users/{}", username);
            result = result.replace(&target, "/Users/<user>");
        }
    }
    result
}

/// Redacts sensitive information such as home directory names and credentials from a text content block.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::redaction::redact_content;
///
/// // Positive case: redacting user home directories and credentials
/// let raw = "path: /Users/user/dev/project, password: \"super_secret_123\"";
/// let expected = "path: /Users/<user>/dev/project, password: \"[REDACTED]\"";
/// assert_eq!(redact_content(raw), expected);
///
/// // Negative case: no sensitive info, remains unchanged
/// assert_eq!(redact_content("hello world"), "hello world");
/// ```
pub fn redact_content(content: &str) -> String {
    let mut result = content.to_string();

    // 1. Redact credential-shaped values, computed against the untouched
    //    `content` (so byte offsets are valid) and applied highest-offset
    //    first (so applying one match never shifts the offsets of another
    //    match still to be applied). Detection logic lives in
    //    `find_credential_matches`, shared with `redact_serialized`.
    let mut cred_matches = find_credential_matches(content);
    cred_matches.sort_by_key(|m| std::cmp::Reverse(m.start));
    for m in cred_matches {
        result.replace_range(m.start..m.end, m.replacement());
    }

    // 2. Redact `/Users/<user>` patterns. `String::replace` rewrites
    //    every occurrence of a given username in one call, so applying this
    //    after step 1 (on the already-credential-redacted `result`) is safe
    //    as long as credential values never contain a "/Users/<user>"
    //    path themselves — the same assumption the original implementation
    //    made. Detection logic lives in `find_path_matches`, shared with
    //    `redact_serialized`.
    for username in find_path_matches(content) {
        let target = format!("/Users/{}", username);
        result = result.replace(&target, "/Users/<user>");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_privacy_redact_paths_and_credentials() {
        let input = "Users home path is /Users/john/dev/proj. Token is secret_token_xyz, and password = \"my-pwd-123\".";
        let output = redact_content(input);
        println!("DEBUG OUTPUT: {}", output);
        assert!(output.contains("/Users/<user>/dev/proj"));
        assert!(output.contains("Token is [REDACTED]"));
        assert!(output.contains("password = \"[REDACTED]\""));
    }
}
