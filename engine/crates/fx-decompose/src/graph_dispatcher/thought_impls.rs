use super::join_contents;
use super::traits::{
    GeneratedThoughts, MergedThought, ThoughtGenerator, ThoughtMerger, ThoughtScore, ThoughtScorer,
};
use crate::{DecomposeError, ThoughtState};
use async_trait::async_trait;
use fx_llm::{completion_text, CompletionRequest, Message, ModelRouter};
use regex::Regex;
use std::sync::Arc;

// --- Configuration constants ---
const DEFAULT_SCORE_FALLBACK: f64 = 0.5;
const SCORE_RESPONSE_TOKEN_LIMIT: u32 = 64;
const GENERATION_TOKEN_LIMIT: u32 = 512;
const MERGE_TOKEN_LIMIT: u32 = 1024;

pub struct LlmThoughtScorer {
    router: Arc<ModelRouter>,
    model: String,
}

impl LlmThoughtScorer {
    pub fn new(router: Arc<ModelRouter>, model: impl Into<String>) -> Self {
        Self {
            router,
            model: model.into(),
        }
    }
}

#[async_trait]
impl ThoughtScorer for LlmThoughtScorer {
    async fn score(
        &self,
        thought: &ThoughtState,
        criteria: &str,
    ) -> Result<ThoughtScore, DecomposeError> {
        let prompt = format!(
            "Rate the following reasoning on a scale of 0.0 to 1.0 based on this criteria:\n\
             {criteria}\n\nReasoning:\n{}\n\nRespond with only a number between 0.0 and 1.0.",
            thought.content
        );
        let response = complete_text_response(
            &self.router,
            &self.model,
            prompt,
            SCORE_RESPONSE_TOKEN_LIMIT,
        )
        .await?;

        Ok(ThoughtScore::new(parse_llm_score(&response), 1))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HeuristicThoughtScorer;

impl HeuristicThoughtScorer {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ThoughtScorer for HeuristicThoughtScorer {
    async fn score(
        &self,
        thought: &ThoughtState,
        criteria: &str,
    ) -> Result<ThoughtScore, DecomposeError> {
        let regex = Regex::new(criteria).map_err(|error| {
            DecomposeError::DecompositionFailed(format!(
                "invalid heuristic scoring pattern {criteria:?}: {error}"
            ))
        })?;
        let value = if regex.is_match(&thought.content) {
            1.0
        } else {
            0.0
        };
        Ok(ThoughtScore::new(value, 0))
    }
}

pub struct LlmThoughtGenerator {
    router: Arc<ModelRouter>,
    model: String,
}

impl LlmThoughtGenerator {
    pub fn new(router: Arc<ModelRouter>, model: impl Into<String>) -> Self {
        Self {
            router,
            model: model.into(),
        }
    }
}

#[async_trait]
impl ThoughtGenerator for LlmThoughtGenerator {
    async fn generate(
        &self,
        parent: &ThoughtState,
        num_branches: usize,
        prompt_override: Option<&str>,
    ) -> Result<GeneratedThoughts, DecomposeError> {
        let mut branches = Vec::with_capacity(num_branches);
        let base_prompt = prompt_override
            .unwrap_or("Generate an alternative reasoning branch for the following thought.");

        for branch_index in 0..num_branches {
            let prompt = format!(
                "{base_prompt}\n\nParent reasoning:\n{}\n\nProduce alternative {}/{} as plain text only.",
                parent.content,
                branch_index + 1,
                num_branches
            );
            branches.push(
                complete_text_response(&self.router, &self.model, prompt, GENERATION_TOKEN_LIMIT)
                    .await?,
            );
        }

        Ok(GeneratedThoughts::new(branches, num_branches))
    }
}

pub struct LlmThoughtMerger {
    router: Arc<ModelRouter>,
    model: String,
}

impl LlmThoughtMerger {
    pub fn new(router: Arc<ModelRouter>, model: impl Into<String>) -> Self {
        Self {
            router,
            model: model.into(),
        }
    }
}

#[async_trait]
impl ThoughtMerger for LlmThoughtMerger {
    async fn merge(
        &self,
        thoughts: &[&ThoughtState],
        instruction: Option<&str>,
    ) -> Result<MergedThought, DecomposeError> {
        let numbered = thoughts
            .iter()
            .enumerate()
            .map(|(index, thought)| format!("Thought {}:\n{}", index + 1, thought.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        let merge_instruction =
            instruction.unwrap_or("Synthesize the strongest ideas into one concise thought.");
        let prompt = format!(
            "{merge_instruction}\n\nMerge the following reasoning paths into one improved thought:\n\n{numbered}"
        );
        let content =
            complete_text_response(&self.router, &self.model, prompt, MERGE_TOKEN_LIMIT).await?;

        Ok(MergedThought::new(content, 1))
    }
}

#[derive(Debug, Clone)]
pub struct ConcatMerger {
    separator: String,
}

impl ConcatMerger {
    pub fn new(separator: impl Into<String>) -> Self {
        Self {
            separator: separator.into(),
        }
    }
}

#[async_trait]
impl ThoughtMerger for ConcatMerger {
    async fn merge(
        &self,
        thoughts: &[&ThoughtState],
        _instruction: Option<&str>,
    ) -> Result<MergedThought, DecomposeError> {
        Ok(MergedThought::new(
            join_contents(thoughts, &self.separator),
            0,
        ))
    }
}

pub(super) fn parse_llm_score(response: &str) -> f64 {
    match first_score_in_response(response) {
        Some(score) => score,
        None => {
            tracing::warn!(
                response,
                "unable to parse llm score; using midpoint fallback"
            );
            DEFAULT_SCORE_FALLBACK
        }
    }
}

fn first_score_in_response(response: &str) -> Option<f64> {
    response
        .split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .filter(|token| !token.is_empty())
        .find_map(|token| {
            token
                .parse::<f64>()
                .ok()
                .filter(|score| (0.0..=1.0).contains(score))
        })
}

async fn complete_text_response(
    router: &Arc<ModelRouter>,
    model: &str,
    prompt: String,
    max_tokens: u32,
) -> Result<String, DecomposeError> {
    let request = CompletionRequest {
        model: model.to_string(),
        messages: vec![Message::user(prompt)],
        tools: Vec::new(),
        temperature: None,
        max_tokens: Some(max_tokens),
        system_prompt: None,
        thinking: None,
    };
    let response = router.complete(request).await.map_err(|error| {
        DecomposeError::DecompositionFailed(format!("thought-model request failed: {error}"))
    })?;
    Ok(completion_text(&response))
}
