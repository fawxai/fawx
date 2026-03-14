use rand::Rng;
use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const CODE_LENGTH: usize = 6;
const MAX_ATTEMPTS: u32 = 5;
const PAIRING_TTL_SECONDS: u64 = 300;
const PAIRING_CHARSET: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ2345679";
const INVALID_PREFIX: &str = "invalid:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairingCode {
    pub code: String,
    pub expires_at: u64,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingError {
    InvalidCode,
    Expired,
    TooManyAttempts,
}

#[derive(Debug, Clone)]
struct PendingPair {
    expires_at: Instant,
    attempts: u32,
}

#[derive(Debug, Default)]
pub struct PairingState {
    codes: HashMap<String, PendingPair>,
}

impl PairingState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn generate(&mut self) -> PairingCode {
        self.generate_with_ttl(PAIRING_TTL_SECONDS)
    }

    pub fn generate_with_ttl(&mut self, ttl_seconds: u64) -> PairingCode {
        let ttl_seconds = normalized_ttl(ttl_seconds);
        self.cleanup_expired();
        let code = self.next_unique_code();
        self.codes.insert(code.clone(), pending_pair(ttl_seconds));
        PairingCode {
            code: format_code(&code),
            expires_at: current_time_seconds().saturating_add(ttl_seconds),
            ttl_seconds,
        }
    }

    pub fn exchange(&mut self, raw_code: &str) -> Result<(), PairingError> {
        let normalized = normalize_code(raw_code).ok_or(PairingError::InvalidCode)?;
        self.cleanup_expired_except(&normalized);
        if let Some(error) = self.validate_known_code(&normalized) {
            return Err(error);
        }
        if self.codes.remove(&normalized).is_some() {
            return Ok(());
        }
        self.record_invalid_attempt(normalized)
    }

    fn next_unique_code(&self) -> String {
        loop {
            let code = generate_code();
            if !self.codes.contains_key(&code) {
                return code;
            }
        }
    }

    fn validate_known_code(&mut self, code: &str) -> Option<PairingError> {
        let pair = self.codes.get(code)?;
        if pair.expires_at <= Instant::now() {
            self.codes.remove(code);
            return Some(PairingError::Expired);
        }
        if pair.attempts >= MAX_ATTEMPTS {
            return Some(PairingError::TooManyAttempts);
        }
        None
    }

    fn record_invalid_attempt(&mut self, code: String) -> Result<(), PairingError> {
        let key = invalid_attempt_key(&code);
        let pair = self
            .codes
            .entry(key)
            .or_insert_with(|| pending_pair(PAIRING_TTL_SECONDS));
        pair.attempts += 1;
        if pair.attempts >= MAX_ATTEMPTS {
            return Err(PairingError::TooManyAttempts);
        }
        Err(PairingError::InvalidCode)
    }

    fn cleanup_expired(&mut self) {
        self.codes
            .retain(|_, pair| pair.expires_at > Instant::now());
    }

    fn cleanup_expired_except(&mut self, code: &str) {
        let invalid_key = invalid_attempt_key(code);
        self.codes.retain(|key, pair| {
            key == code || key == &invalid_key || pair.expires_at > Instant::now()
        });
    }
}

fn generate_code() -> String {
    let charset = PAIRING_CHARSET.as_bytes();
    let mut rng = rand::thread_rng();
    (0..CODE_LENGTH)
        .map(|_| {
            let index = rng.gen_range(0..charset.len());
            char::from(charset[index])
        })
        .collect()
}

fn normalize_code(raw_code: &str) -> Option<String> {
    let normalized = raw_code
        .chars()
        .filter(|ch| *ch != '-' && !ch.is_ascii_whitespace())
        .map(|ch| ch.to_ascii_uppercase())
        .collect::<String>();

    if normalized.len() != CODE_LENGTH {
        return None;
    }
    normalized
        .chars()
        .all(|ch| PAIRING_CHARSET.contains(ch))
        .then_some(normalized)
}

fn format_code(code: &str) -> String {
    let Some(normalized) = normalize_code(code) else {
        return code.to_string();
    };
    let (head, tail) = normalized.split_at(3);
    format!("{head}-{tail}")
}

fn pending_pair(ttl_seconds: u64) -> PendingPair {
    PendingPair {
        expires_at: Instant::now() + Duration::from_secs(ttl_seconds),
        attempts: 0,
    }
}

fn normalized_ttl(ttl_seconds: u64) -> u64 {
    ttl_seconds.max(1)
}

fn current_time_seconds() -> u64 {
    crate::time_util::current_time_seconds()
}

fn invalid_attempt_key(code: &str) -> String {
    format!("{INVALID_PREFIX}{code}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn generate_code_format_valid() {
        let code = generate_code();

        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|ch| PAIRING_CHARSET.contains(ch)));
        assert_eq!(format_code(&code).chars().nth(3), Some('-'));
        assert_eq!(normalize_code("abc-234"), Some("ABC234".to_string()));
        assert_eq!(normalize_code("abc-12o"), None);
    }

    #[test]
    fn generate_with_custom_ttl_includes_expiry_metadata() {
        let mut state = PairingState::new();
        let before = current_time_seconds();

        let pair = state.generate_with_ttl(123);

        assert_eq!(pair.ttl_seconds, 123);
        assert!(pair.expires_at >= before + 123);
        assert!(pair.expires_at <= current_time_seconds() + 123);
    }

    #[test]
    fn generate_zero_ttl_is_clamped_to_one_second() {
        let mut state = PairingState::new();

        let pair = state.generate_with_ttl(0);

        assert_eq!(pair.ttl_seconds, 1);
    }

    #[test]
    fn exchange_happy_path_consumes_code() {
        let mut state = PairingState::new();
        let pair = state.generate();

        assert!(state.exchange(&pair.code).is_ok());
        assert_eq!(state.exchange(&pair.code), Err(PairingError::InvalidCode));
    }

    #[test]
    fn exchange_expired_code_fails() {
        let mut state = PairingState::new();
        state.codes.insert(
            "ABC234".to_string(),
            PendingPair {
                expires_at: Instant::now() - Duration::from_secs(1),
                attempts: 0,
            },
        );

        assert_eq!(state.exchange("ABC-234"), Err(PairingError::Expired));
        assert!(!state.codes.contains_key("ABC234"));
    }

    #[test]
    fn exchange_wrong_code_attempts_are_tracked() {
        let mut state = PairingState::new();

        assert_eq!(state.exchange("ZZZ-999"), Err(PairingError::InvalidCode));
        let attempt = state
            .codes
            .get(&invalid_attempt_key("ZZZ999"))
            .expect("attempt tracker");
        assert_eq!(attempt.attempts, 1);
    }

    #[test]
    fn exchange_max_attempts_locks_code() {
        let mut state = PairingState::new();

        for _ in 0..4 {
            assert_eq!(state.exchange("ZZZ-999"), Err(PairingError::InvalidCode));
        }
        assert_eq!(
            state.exchange("ZZZ-999"),
            Err(PairingError::TooManyAttempts)
        );
        assert_eq!(
            state.exchange("ZZZ-999"),
            Err(PairingError::TooManyAttempts)
        );
    }

    #[test]
    fn concurrent_codes_stay_independent() {
        let mut state = PairingState::new();
        let first = state.generate();
        let second = state.generate();

        assert_ne!(first.code, second.code);
        assert!(state.exchange(&first.code).is_ok());
        assert!(state.exchange(&second.code).is_ok());
    }

    #[test]
    fn generate_cleans_up_expired_codes() {
        let mut state = PairingState::new();
        state.codes.insert(
            "ABC234".to_string(),
            PendingPair {
                expires_at: Instant::now() - Duration::from_secs(1),
                attempts: 0,
            },
        );

        let _ = state.generate();

        assert!(!state.codes.contains_key("ABC234"));
    }
}
