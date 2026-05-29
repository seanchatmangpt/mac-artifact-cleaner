//! Privacy and redaction helper rules.
//!
//! Provides functions to identify and sanitize sensitive local information (e.g. user home paths and credentials)
//! before reports or plans are output for sharing or version control.

/// Redacts a path to hide local user profiles in examples.
///
/// # Examples
///
/// ```
/// use pentecost::domain::redaction::redact_path;
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
/// use pentecost::domain::redaction::redact_content;
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

    // 1. Redact `/Users/<user>` patterns
    let mut search_idx = 0;
    while let Some(start_idx) = result[search_idx..].find("/Users/") {
        let abs_start = search_idx + start_idx;
        let after_users = &result[abs_start + 7..];
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
        if !username.is_empty() && username != "<user>" {
            let target = format!("/Users/{}", username);
            result = result.replace(&target, "/Users/<user>");
        }
        search_idx = abs_start + 7;
        if search_idx >= result.len() {
            break;
        }
    }

    // 2. Redact credential patterns (case-insensitive keys)
    let credential_keys = [
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

    for key in &credential_keys {
        let mut key_idx = 0;
        loop {
            let lower_result = result.to_lowercase();
            if let Some(found_idx) = lower_result[key_idx..].find(key) {
                let abs_found = key_idx + found_idx;
                let after_key = &result[abs_found + key.len()..];

                let mut temp_idx = 0;
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
                            || after_is
                                .starts_with(|c: char| c.is_whitespace() || c == ':' || c == '=')
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

                if sep_len > 0 {
                    let val_start = abs_found + key.len() + sep_len;
                    if val_start < result.len() {
                        let val_str = &result[val_start..];
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

                        if val_len > 0 {
                            let extracted_val = &result[val_start..val_start + val_len];
                            if extracted_val.contains("[REDACTED") {
                                key_idx = abs_found + key.len();
                                continue;
                            }
                            let trimmed_val = extracted_val
                                .trim()
                                .trim_matches('"')
                                .trim_matches('\'')
                                .to_lowercase();
                            let is_another_key = credential_keys.iter().any(|k| {
                                trimmed_val == *k
                                    || trimmed_val.starts_with(&format!("{}:", k))
                                    || trimmed_val.starts_with(&format!("{}=", k))
                                    || (trimmed_val.ends_with(':')
                                        && credential_keys
                                            .contains(&trimmed_val.trim_end_matches(':')))
                            });
                            if is_another_key {
                                key_idx = abs_found + key.len();
                                continue;
                            }
                            let redacted_val = if quotes_char.is_some() {
                                "\"[REDACTED]\"".to_string()
                            } else {
                                "[REDACTED]".to_string()
                            };
                            result.replace_range(val_start..val_start + val_len, &redacted_val);
                            // Restart scanning from the beginning of the string since its indices shifted
                            key_idx = 0;
                            continue;
                        }
                    }
                }
                key_idx = abs_found + key.len();
                if key_idx >= result.len() {
                    break;
                }
            } else {
                break;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_privacy_redact_paths_and_credentials() {
        let input = "Users home path is /Users/sac/dev/proj. Token is secret_token_xyz, and password = \"my-pwd-123\".";
        let output = redact_content(input);
        println!("DEBUG OUTPUT: {}", output);
        assert!(output.contains("/Users/<user>/dev/proj"));
        assert!(output.contains("Token is [REDACTED]"));
        assert!(output.contains("password = \"[REDACTED]\""));
    }
}
