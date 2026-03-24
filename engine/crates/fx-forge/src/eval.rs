use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalConfig {
    pub prompts: Vec<EvalPrompt>,
    pub compare_to_base: bool,
    pub llm_judge: Option<LlmJudgeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalPrompt {
    pub system: String,
    pub user: String,
    pub expected_behavior: String,
    pub expected_contains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmJudgeConfig {
    pub judge_model: String,
    pub rubric: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalResults {
    pub improved: usize,
    pub regressed: usize,
    pub neutral: usize,
    pub avg_quality_delta: f64,
    pub prompts_evaluated: usize,
    pub judge_scores: Option<Vec<f64>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_config_roundtrip() {
        let config = EvalConfig {
            prompts: vec![EvalPrompt {
                system: "sys".to_owned(),
                user: "prompt".to_owned(),
                expected_behavior: "should fix the bug".to_owned(),
                expected_contains: vec!["fix".to_owned()],
            }],
            compare_to_base: true,
            llm_judge: Some(LlmJudgeConfig {
                judge_model: "claude-opus-4-6".to_owned(),
                rubric: "Rate quality 1-5".to_owned(),
            }),
        };
        let json = serde_json::to_string(&config).unwrap();
        let decoded: EvalConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.prompts.len(), 1);
        assert!(decoded.llm_judge.is_some());
    }

    #[test]
    fn eval_results_roundtrip() {
        let results = EvalResults {
            improved: 3,
            regressed: 1,
            neutral: 2,
            avg_quality_delta: 0.3,
            prompts_evaluated: 6,
            judge_scores: Some(vec![4.0, 3.5, 4.5]),
        };
        let json = serde_json::to_string(&results).unwrap();
        let decoded: EvalResults = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, results);
    }
}
