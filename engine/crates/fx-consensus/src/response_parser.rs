use crate::{ConsensusError, PatchResponse};
use std::collections::BTreeMap;

pub(crate) const PATCH_START: &str = "<PATCH>";
pub(crate) const PATCH_END: &str = "</PATCH>";
pub(crate) const APPROACH_START: &str = "<APPROACH>";
pub(crate) const APPROACH_END: &str = "</APPROACH>";
pub(crate) const METRICS_START: &str = "<METRICS>";
pub(crate) const METRICS_END: &str = "</METRICS>";
pub(crate) const METRIC_KEYS: [&str; 3] = ["build_success", "test_pass_rate", "signal_resolution"];

pub(crate) fn parse_patch_response(text: &str) -> Result<PatchResponse, ConsensusError> {
    let patch = extract_patch(text).ok_or_else(|| {
        ConsensusError::Protocol("generated response did not include a diff patch".to_owned())
    })?;
    let approach = extract_approach(text, &patch);
    let self_metrics = extract_metrics(text);
    Ok(PatchResponse {
        patch,
        approach,
        self_metrics,
    })
}

pub(crate) fn extract_patch(text: &str) -> Option<String> {
    extract_tagged_block(text, PATCH_START, PATCH_END)
        .or_else(|| extract_fenced_block(text, "diff"))
        .or_else(|| extract_fenced_block(text, "patch"))
}

pub(crate) fn extract_fenced_block(text: &str, language: &str) -> Option<String> {
    let fence = format!("```{language}");
    let start = text.find(&fence)?;
    let after_start = &text[start + fence.len()..];
    let end = after_start.find("```")?;
    Some(after_start[..end].trim().to_owned())
}

pub(crate) fn extract_tagged_block(text: &str, start_tag: &str, end_tag: &str) -> Option<String> {
    let start = text.find(start_tag)? + start_tag.len();
    let end = text[start..].find(end_tag)? + start;
    Some(text[start..end].trim().to_owned())
}

pub(crate) fn extract_approach(text: &str, patch: &str) -> String {
    if let Some(approach) = extract_tagged_block(text, APPROACH_START, APPROACH_END) {
        return fallback_approach(&approach);
    }

    let mut remainder = text.trim().to_owned();
    if let Some(tagged_patch) = extract_tagged_block(text, PATCH_START, PATCH_END) {
        let wrapped = format!("{PATCH_START}\n{tagged_patch}\n{PATCH_END}");
        remainder = remainder.replacen(&wrapped, "", 1);
    } else {
        remainder = remainder.replacen(patch, "", 1);
        remainder = remainder
            .replace("```diff", "")
            .replace("```patch", "")
            .replace("```", "");
    }
    if let Some(metrics_block) = extract_json_block(text) {
        remainder = remainder.replacen(&metrics_block, "", 1);
    }
    fallback_approach(&remainder)
}

pub(crate) fn fallback_approach(text: &str) -> String {
    let approach = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if approach.is_empty() {
        "No approach summary provided".to_owned()
    } else {
        approach
    }
}

pub(crate) fn extract_metrics(text: &str) -> BTreeMap<String, f64> {
    let Some(metrics_block) = extract_json_block(text) else {
        return BTreeMap::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&metrics_block) else {
        return BTreeMap::new();
    };
    let Some(object) = value.as_object() else {
        return BTreeMap::new();
    };
    object
        .iter()
        .filter_map(|(key, value)| value.as_f64().map(|number| (key.clone(), number)))
        .collect()
}

pub(crate) fn extract_json_block(text: &str) -> Option<String> {
    if let Some(tagged) = extract_tagged_block(text, METRICS_START, METRICS_END) {
        return Some(tagged);
    }

    let search_start = patch_search_start(text);
    let search_text = &text[search_start..];
    let mut candidate_starts = search_text.match_indices('{').collect::<Vec<_>>();
    candidate_starts.reverse();

    for (relative_start, _) in candidate_starts {
        let absolute_start = search_start + relative_start;
        if let Some(block) = extract_balanced_json(text, absolute_start) {
            if has_expected_metrics(&block) {
                return Some(block);
            }
        }
    }
    None
}

pub(crate) fn patch_search_start(text: &str) -> usize {
    if let Some(end) = tagged_block_end(text, PATCH_START, PATCH_END) {
        return end;
    }
    for language in ["diff", "patch"] {
        if let Some(end) = fenced_block_end(text, language) {
            return end;
        }
    }
    0
}

pub(crate) fn tagged_block_end(text: &str, start_tag: &str, end_tag: &str) -> Option<usize> {
    let start = text.find(start_tag)? + start_tag.len();
    let end = text[start..].find(end_tag)? + start;
    Some(end + end_tag.len())
}

pub(crate) fn fenced_block_end(text: &str, language: &str) -> Option<usize> {
    let fence = format!("```{language}");
    let start = text.find(&fence)? + fence.len();
    let end = text[start..].find("```")? + start;
    Some(end + 3)
}

pub(crate) fn extract_balanced_json(text: &str, start: usize) -> Option<String> {
    let mut depth = 0_u32;
    let mut end = None;
    for (offset, character) in text[start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = Some(start + offset + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    end.map(|index| text[start..index].to_owned())
}

pub(crate) fn has_expected_metrics(block: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(block) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    METRIC_KEYS.iter().all(|key| object.contains_key(*key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_block_prefers_metrics_after_patch_content() {
        let text = concat!(
            "```diff\n",
            "diff --git a/src/config.rs b/src/config.rs\n",
            "--- a/src/config.rs\n",
            "+++ b/src/config.rs\n",
            "@@ -1 +1 @@\n",
            "-const DEFAULT: &str = \"{\\\"build_success\\\":0.1}\";\n",
            "+const DEFAULT: &str = \"still not metrics\";\n",
            "```\n",
            "Approach: keep the diff stable.\n",
            "{\"build_success\":1.0,\"test_pass_rate\":0.75,\"signal_resolution\":0.5}"
        );

        assert_eq!(
            extract_json_block(text),
            Some(
                "{\"build_success\":1.0,\"test_pass_rate\":0.75,\"signal_resolution\":0.5}"
                    .to_owned()
            )
        );
    }
}
