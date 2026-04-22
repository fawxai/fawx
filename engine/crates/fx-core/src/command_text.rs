use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CommandTokenizationError {
    #[error("command ends with trailing escape")]
    TrailingEscape,
    #[error("command contains unmatched single quote")]
    UnmatchedSingleQuote,
    #[error("command contains unmatched double quote")]
    UnmatchedDoubleQuote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteState {
    Single,
    Double,
}

pub fn tokenize_non_shell_command(command: &str) -> Result<Vec<String>, CommandTokenizationError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut token_started = false;
    let mut chars = command.chars().peekable();
    let mut quote_state = None;

    while let Some(ch) = chars.next() {
        match quote_state {
            Some(QuoteState::Single) => {
                if ch == '\'' {
                    quote_state = None;
                } else {
                    current.push(ch);
                }
                token_started = true;
            }
            Some(QuoteState::Double) => {
                match ch {
                    '"' => quote_state = None,
                    '\\' => match chars.next() {
                        Some(next @ ('"' | '\\' | '$' | '`')) => current.push(next),
                        Some(next) => {
                            current.push('\\');
                            current.push(next);
                        }
                        None => return Err(CommandTokenizationError::TrailingEscape),
                    },
                    _ => current.push(ch),
                }
                token_started = true;
            }
            None => match ch {
                '\'' => {
                    quote_state = Some(QuoteState::Single);
                    token_started = true;
                }
                '"' => {
                    quote_state = Some(QuoteState::Double);
                    token_started = true;
                }
                '\\' => match chars.next() {
                    Some(next) => {
                        current.push(next);
                        token_started = true;
                    }
                    None => return Err(CommandTokenizationError::TrailingEscape),
                },
                _ if ch.is_whitespace() => {
                    if token_started {
                        tokens.push(std::mem::take(&mut current));
                        token_started = false;
                    }
                }
                _ => {
                    current.push(ch);
                    token_started = true;
                }
            },
        }
    }

    match quote_state {
        Some(QuoteState::Single) => Err(CommandTokenizationError::UnmatchedSingleQuote),
        Some(QuoteState::Double) => Err(CommandTokenizationError::UnmatchedDoubleQuote),
        None => {
            if token_started {
                tokens.push(current);
            }
            Ok(tokens)
        }
    }
}

pub fn normalize_http_url_token(token: &str) -> Option<String> {
    let trimmed = token.trim();
    let trimmed = trimmed.trim_start_matches(['"', '\'', '`', '<', '(', '[', '{']);
    let trimmed = trim_trailing_url_delimiters(trimmed);
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn trim_trailing_url_delimiters(token: &str) -> &str {
    let trimmed = trim_trailing_url_wrappers(token);

    let trimmed = if let Some(last) = trimmed.chars().last() {
        if matches!(last, ',' | '.' | ';' | ':' | '!' | '?') {
            &trimmed[..trimmed.len() - last.len_utf8()]
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    trim_trailing_url_wrappers(trimmed)
}

fn trim_trailing_url_wrappers(token: &str) -> &str {
    let mut trimmed = token;
    while let Some(last) = trimmed.chars().last() {
        if matches!(last, '"' | '\'' | '`' | '>' | ')' | ']' | '}') {
            trimmed = &trimmed[..trimmed.len() - last.len_utf8()];
            continue;
        }
        break;
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_non_shell_command_preserves_quoted_arguments() {
        let tokens = tokenize_non_shell_command(r#"open -a "Google Chrome" --new"#)
            .expect("quoted command should parse");

        assert_eq!(
            tokens,
            vec![
                "open".to_string(),
                "-a".to_string(),
                "Google Chrome".to_string(),
                "--new".to_string(),
            ]
        );
    }

    #[test]
    fn tokenize_non_shell_command_rejects_unmatched_quotes() {
        let error = tokenize_non_shell_command(r#"open -a "Google Chrome"#)
            .expect_err("unterminated quote should fail");

        assert_eq!(error, CommandTokenizationError::UnmatchedDoubleQuote);
    }

    #[test]
    fn tokenize_non_shell_command_handles_posix_single_quote_splicing() {
        let tokens = tokenize_non_shell_command(r#"open -a 'it'\''s here' --new"#)
            .expect("single-quote splice should parse");

        assert_eq!(
            tokens,
            vec![
                "open".to_string(),
                "-a".to_string(),
                "it's here".to_string(),
                "--new".to_string(),
            ]
        );
    }

    #[test]
    fn normalize_http_url_token_trims_balanced_wrappers_and_sentence_punctuation() {
        assert_eq!(
            normalize_http_url_token(r#"<"https://example.com/path?x=1.">"#),
            Some("https://example.com/path?x=1".to_string())
        );
        assert_eq!(
            normalize_http_url_token("https://example.com..."),
            Some("https://example.com..".to_string())
        );
        assert_eq!(
            normalize_http_url_token("`https://example.com/path`;"),
            Some("https://example.com/path".to_string())
        );
        assert_eq!(normalize_http_url_token("knowledge"), None);
    }
}
