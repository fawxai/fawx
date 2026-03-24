use crate::config::ImprovementConfig;
use crate::error::ImprovementError;
use fx_analysis::{AnalysisFinding, Confidence};
use ring::digest;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ImprovementCandidate {
    pub finding: AnalysisFinding,
    pub fingerprint: String,
}

pub struct ImprovementDetector {
    config: ImprovementConfig,
    known_fingerprints: HashSet<String>,
    history_path: PathBuf,
}

#[derive(Deserialize)]
struct HistoryEntry {
    fingerprint: String,
}

impl ImprovementDetector {
    pub fn new(config: ImprovementConfig, data_dir: &Path) -> Result<Self, ImprovementError> {
        config.validate()?;
        let history_dir = data_dir.join("improvements");
        let history_path = history_dir.join("history.jsonl");
        let known_fingerprints = load_history(&history_path)?;
        Ok(Self {
            config,
            known_fingerprints,
            history_path,
        })
    }

    pub fn detect(&self, findings: &[AnalysisFinding]) -> Vec<ImprovementCandidate> {
        findings
            .iter()
            .filter(|finding| {
                confidence_meets_threshold(finding.confidence, self.config.min_confidence)
            })
            .filter(|finding| finding.evidence.len() >= self.config.min_evidence_count)
            .filter(|finding| finding.suggested_action.is_some())
            .map(|finding| ImprovementCandidate {
                fingerprint: compute_fingerprint(&finding.pattern_name, &finding.description),
                finding: finding.clone(),
            })
            .filter(|candidate| !self.known_fingerprints.contains(&candidate.fingerprint))
            .take(self.config.max_improvements_per_run)
            .collect()
    }

    pub fn record_acted(&mut self, fingerprint: &str) -> Result<(), ImprovementError> {
        if let Some(parent) = self.history_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                ImprovementError::History(format!("create history dir: {error}"))
            })?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.history_path)
            .map_err(|error| ImprovementError::History(format!("open history: {error}")))?;
        writeln!(file, "{}", serde_json::json!({"fingerprint": fingerprint}))
            .map_err(|error| ImprovementError::History(format!("write history: {error}")))?;
        self.known_fingerprints.insert(fingerprint.to_string());
        Ok(())
    }
}

fn load_history(path: &Path) -> Result<HashSet<String>, ImprovementError> {
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let file = fs::File::open(path)
        .map_err(|error| ImprovementError::History(format!("open history: {error}")))?;
    let reader = std::io::BufReader::new(file);
    let mut fingerprints = HashSet::new();

    for (line_number, line) in reader.lines().enumerate() {
        let line =
            line.map_err(|error| ImprovementError::History(format!("read line: {error}")))?;
        if let Some(fingerprint) = parse_history_line(&line, line_number + 1)? {
            fingerprints.insert(fingerprint);
        }
    }
    Ok(fingerprints)
}

fn parse_history_line(line: &str, line_number: usize) -> Result<Option<String>, ImprovementError> {
    if line.trim().is_empty() {
        return Ok(None);
    }
    let entry: HistoryEntry = serde_json::from_str(line).map_err(|error| {
        ImprovementError::History(format!("parse history line {line_number}: {error}"))
    })?;
    Ok(Some(entry.fingerprint))
}

pub(crate) fn compute_fingerprint(pattern_name: &str, description: &str) -> String {
    let input = format!("{pattern_name}:{description}");
    let hash = digest::digest(&digest::SHA256, input.as_bytes());
    hash.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn confidence_meets_threshold(actual: Confidence, minimum: Confidence) -> bool {
    confidence_rank(actual) >= confidence_rank(minimum)
}

fn confidence_rank(confidence: Confidence) -> u8 {
    match confidence {
        Confidence::High => 3,
        Confidence::Medium => 2,
        Confidence::Low => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_analysis::{Confidence, SignalEvidence};
    use fx_core::signals::SignalKind;
    use tempfile::TempDir;

    fn mk_finding(
        name: &str,
        confidence: Confidence,
        evidence_count: usize,
        has_action: bool,
    ) -> AnalysisFinding {
        let evidence: Vec<SignalEvidence> = (0..evidence_count)
            .map(|index| SignalEvidence {
                session_id: format!("sess-{index}"),
                signal_kind: SignalKind::Friction,
                message: "test".to_string(),
                timestamp_ms: index as u64,
            })
            .collect();
        AnalysisFinding {
            pattern_name: name.to_string(),
            description: format!("Description for {name}"),
            confidence,
            evidence,
            suggested_action: if has_action {
                Some("fix it".to_string())
            } else {
                None
            },
        }
    }

    #[test]
    fn filters_below_confidence_threshold() {
        let tmp = TempDir::new().unwrap();
        let detector = ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();
        let findings = vec![mk_finding("low-conf", Confidence::Low, 5, true)];
        let candidates = detector.detect(&findings);
        assert!(candidates.is_empty());
    }

    #[test]
    fn filters_insufficient_evidence() {
        let tmp = TempDir::new().unwrap();
        let detector = ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();
        let findings = vec![mk_finding("few-evidence", Confidence::High, 1, true)];
        let candidates = detector.detect(&findings);
        assert!(candidates.is_empty());
    }

    #[test]
    fn filters_without_suggested_action() {
        let tmp = TempDir::new().unwrap();
        let detector = ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();
        let findings = vec![mk_finding("no-action", Confidence::High, 5, false)];
        let candidates = detector.detect(&findings);
        assert!(candidates.is_empty());
    }

    #[test]
    fn filters_known_fingerprints() {
        let tmp = TempDir::new().unwrap();
        let mut detector =
            ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();
        let finding = mk_finding("known", Confidence::High, 5, true);
        let fingerprint = compute_fingerprint(&finding.pattern_name, &finding.description);
        detector.record_acted(&fingerprint).unwrap();
        let candidates = detector.detect(&[finding]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn respects_max_improvements_per_run() {
        let tmp = TempDir::new().unwrap();
        let config = ImprovementConfig {
            max_improvements_per_run: 1,
            ..ImprovementConfig::default()
        };
        let detector = ImprovementDetector::new(config, tmp.path()).unwrap();
        let findings = vec![
            mk_finding("a", Confidence::High, 5, true),
            mk_finding("b", Confidence::High, 5, true),
        ];
        let candidates = detector.detect(&findings);
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let first = compute_fingerprint("pattern", "desc");
        let second = compute_fingerprint("pattern", "desc");
        assert_eq!(first, second);
    }

    #[test]
    fn fingerprint_differs_for_different_findings() {
        let first = compute_fingerprint("pattern-a", "desc-a");
        let second = compute_fingerprint("pattern-b", "desc-b");
        assert_ne!(first, second);
    }

    #[test]
    fn record_acted_persists_to_disk() {
        let tmp = TempDir::new().unwrap();
        let mut detector =
            ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();
        detector.record_acted("test-fp").unwrap();

        let history = tmp.path().join("improvements").join("history.jsonl");
        let content = std::fs::read_to_string(&history).unwrap();
        assert!(content.contains("test-fp"));
    }

    #[test]
    fn detect_with_empty_findings_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let detector = ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();
        assert!(detector.detect(&[]).is_empty());
    }

    #[test]
    fn passes_all_filters() {
        let tmp = TempDir::new().unwrap();
        let detector = ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();
        let findings = vec![mk_finding("good", Confidence::High, 5, true)];
        let candidates = detector.detect(&findings);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].finding.pattern_name, "good");
    }

    #[test]
    fn malformed_history_line_returns_error() {
        let tmp = TempDir::new().unwrap();
        let history_dir = tmp.path().join("improvements");
        std::fs::create_dir_all(&history_dir).unwrap();
        let history_path = history_dir.join("history.jsonl");
        std::fs::write(&history_path, "{\"fingerprint\":\"ok\"}\n{not-json}\n").unwrap();

        let result = ImprovementDetector::new(ImprovementConfig::default(), tmp.path());
        assert!(result.is_err(), "malformed history should fail loudly");
        let error = result.err().unwrap();
        assert!(matches!(error, ImprovementError::History(message) if message.contains("line 2")));
    }
}
