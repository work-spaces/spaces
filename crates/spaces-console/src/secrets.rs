use std::sync::Arc;

pub(crate) const DEFAULT_MAX_REDACTIONS: usize = 16;
pub(crate) const DEFAULT_MIN_SECRET_LENGTH: usize = 4;

#[derive(Debug, Clone)]
pub struct Secrets {
    pub secrets: Vec<Arc<str>>,
    pub redacted: Arc<str>,
    pub max_redactions: usize,
    pub min_secret_length: usize,
}

impl Default for Secrets {
    fn default() -> Self {
        Self {
            secrets: Vec::new(),
            redacted: "<REDACTED>".into(),
            max_redactions: DEFAULT_MAX_REDACTIONS,
            min_secret_length: DEFAULT_MIN_SECRET_LENGTH,
        }
    }
}

impl Secrets {
    pub fn redact(&self, text: Arc<str>) -> Arc<str> {
        if self.secrets.is_empty() {
            text
        } else {
            let mut result = text.to_string();
            for secret in &self.secrets {
                if !secret.is_empty() && secret.len() >= self.min_secret_length {
                    result = result.replacen(
                        secret.as_ref(),
                        self.redacted.as_ref(),
                        self.max_redactions,
                    );
                    if let Some(pos) = result.find(secret.as_ref()) {
                        result.truncate(pos);
                        result.push_str("...");
                    }
                }
            }
            result.into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_skips_empty_secrets() {
        let secrets = Secrets {
            secrets: vec!["".into()],
            redacted: "REDACTED".into(),
            max_redactions: usize::MAX,
            min_secret_length: 0,
        };
        let input: Arc<str> = "hello world".into();
        let result = secrets.redact(input.clone());
        assert_eq!(result, input, "Empty secret should not alter the text");
    }

    #[test]
    fn redact_skips_empty_secrets_among_valid_ones() {
        let secrets = Secrets {
            secrets: vec!["".into(), "world".into()],
            redacted: "REDACTED".into(),
            max_redactions: usize::MAX,
            min_secret_length: 0,
        };
        let input: Arc<str> = "hello world".into();
        let result = secrets.redact(input);
        assert_eq!(
            result.as_ref(),
            "hello REDACTED",
            "Only the non-empty secret should be redacted"
        );
    }

    #[test]
    fn redact_with_no_secrets() {
        let secrets = Secrets {
            secrets: vec![],
            redacted: "REDACTED".into(),
            max_redactions: usize::MAX,
            min_secret_length: 0,
        };
        let input: Arc<str> = "hello world".into();
        let result = secrets.redact(input.clone());
        assert_eq!(result, input, "No secrets means text is returned unchanged");
    }

    #[test]
    fn redact_replaces_valid_secret() {
        let secrets = Secrets {
            secrets: vec!["secret_token".into()],
            redacted: "REDACTED".into(),
            max_redactions: usize::MAX,
            min_secret_length: 0,
        };
        let input: Arc<str> = "my secret_token is here".into();
        let result = secrets.redact(input);
        assert_eq!(result.as_ref(), "my REDACTED is here");
    }

    #[test]
    fn redact_all_empty_secrets_leaves_text_unchanged() {
        let secrets = Secrets {
            secrets: vec!["".into(), "".into(), "".into()],
            redacted: "REDACTED".into(),
            max_redactions: usize::MAX,
            min_secret_length: 0,
        };
        let input: Arc<str> = "nothing should change".into();
        let result = secrets.redact(input.clone());
        assert_eq!(
            result, input,
            "All-empty secrets list should not alter the text"
        );
    }

    #[test]
    fn redact_max_redactions_limits_replacements() {
        let secrets = Secrets {
            secrets: vec!["secret".into()],
            redacted: "REDACTED".into(),
            max_redactions: 2,
            min_secret_length: 0,
        };
        let input: Arc<str> = "secret secret secret secret".into();
        let result = secrets.redact(input);
        assert_eq!(
            result.as_ref(),
            "REDACTED REDACTED ...",
            "Only the first max_redactions occurrences should be replaced, remainder truncated"
        );
    }

    #[test]
    fn redact_max_redactions_zero_truncates_at_first_occurrence() {
        let secrets = Secrets {
            secrets: vec!["secret".into()],
            redacted: "REDACTED".into(),
            max_redactions: 0,
            min_secret_length: 0,
        };
        let input: Arc<str> = "secret secret".into();
        let result = secrets.redact(input);
        assert_eq!(
            result.as_ref(),
            "...",
            "max_redactions=0 replaces nothing, so secret is immediately truncated"
        );
    }

    #[test]
    fn redact_truncates_after_max_redactions() {
        let secrets = Secrets {
            secrets: vec!["secret".into()],
            redacted: "REDACTED".into(),
            max_redactions: 1,
            min_secret_length: 0,
        };
        let input: Arc<str> = "before secret after secret trailing".into();
        let result = secrets.redact(input);
        assert_eq!(
            result.as_ref(),
            "before REDACTED after ...",
            "Text before the unredacted occurrence should be preserved, then truncated with ..."
        );
    }

    #[test]
    fn redact_min_secret_length_skips_short_secrets() {
        let secrets = Secrets {
            secrets: vec!["ab".into(), "longersecret".into()],
            redacted: "REDACTED".into(),
            max_redactions: usize::MAX,
            min_secret_length: 5,
        };
        let input: Arc<str> = "ab longersecret".into();
        let result = secrets.redact(input);
        assert_eq!(
            result.as_ref(),
            "ab REDACTED",
            "Secrets shorter than min_secret_length should not be redacted"
        );
    }

    #[test]
    fn redact_min_secret_length_exact_boundary() {
        let secrets = Secrets {
            secrets: vec!["abc".into(), "abcd".into()],
            redacted: "REDACTED".into(),
            max_redactions: usize::MAX,
            min_secret_length: 4,
        };
        let input: Arc<str> = "abc abcd".into();
        let result = secrets.redact(input);
        assert_eq!(
            result.as_ref(),
            "abc REDACTED",
            "Secret with length == min_secret_length should be redacted, shorter should not"
        );
    }
}
