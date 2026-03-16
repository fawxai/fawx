//! DAG parser for Custom aggregation strategy.
//!
//! The DAG spec is a string describing execution order:
//! - Comma-separated indices run in **parallel** within a level
//! - `->` separates **sequential** levels
//!
//! # Examples
//! - `"0->1->2"` — fully sequential (goal 0, then 1, then 2)
//! - `"0,1,2"` — fully parallel (all three at once)
//! - `"0,1->2->3"` — goals 0 and 1 in parallel, then 2, then 3
//! - `"0,1->2,3->4"` — {0,1} parallel, then {2,3} parallel, then {4}
//!
//! All indices must be 0-based, unique, and cover every sub-goal exactly once.

use crate::error::DecomposeError;
use std::collections::HashSet;

/// Execution DAG for Custom aggregation strategy.
/// Levels execute sequentially; goals within a level execute in parallel.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionDag {
    levels: Vec<Vec<usize>>,
}

impl ExecutionDag {
    /// Parse a DAG spec string.
    ///
    /// Format: comma-separated indices for parallel, `->` for sequential.
    /// Example: `"0,1->2->3"` = {0,1} in parallel, then {2}, then {3}.
    pub fn parse(spec: &str, sub_goal_count: usize) -> Result<Self, DecomposeError> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err(DecomposeError::DagParseError(
                "empty DAG specification".to_owned(),
            ));
        }
        let levels = parse_levels(spec, sub_goal_count)?;
        validate_coverage(&levels, sub_goal_count)?;
        Ok(Self { levels })
    }

    pub fn levels(&self) -> &[Vec<usize>] {
        &self.levels
    }
}

fn parse_levels(spec: &str, sub_goal_count: usize) -> Result<Vec<Vec<usize>>, DecomposeError> {
    spec.split("->")
        .map(|level_str| parse_level(level_str.trim(), sub_goal_count))
        .collect()
}

fn parse_level(level_str: &str, sub_goal_count: usize) -> Result<Vec<usize>, DecomposeError> {
    level_str
        .split(',')
        .map(|index_str| parse_index(index_str.trim(), sub_goal_count))
        .collect()
}

fn parse_index(index_str: &str, sub_goal_count: usize) -> Result<usize, DecomposeError> {
    let index: usize = index_str
        .parse()
        .map_err(|_| DecomposeError::DagParseError(format!("invalid index: {index_str:?}")))?;
    if index >= sub_goal_count {
        return Err(DecomposeError::DagParseError(format!(
            "index {index} out of range (max {})",
            sub_goal_count.saturating_sub(1)
        )));
    }
    Ok(index)
}

fn validate_coverage(levels: &[Vec<usize>], sub_goal_count: usize) -> Result<(), DecomposeError> {
    let mut seen = HashSet::new();
    for level in levels {
        for &index in level {
            if !seen.insert(index) {
                return Err(DecomposeError::DagParseError(format!(
                    "duplicate index: {index}"
                )));
            }
        }
    }
    if seen.len() != sub_goal_count {
        return Err(DecomposeError::DagParseError(format!(
            "DAG covers {} indices but expected {sub_goal_count}",
            seen.len()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_sequence() {
        let dag = ExecutionDag::parse("0->1->2", 3).unwrap();
        assert_eq!(dag.levels(), &[vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn parse_parallel_then_sequential() {
        let dag = ExecutionDag::parse("0,1->2->3", 4).unwrap();
        assert_eq!(dag.levels(), &[vec![0, 1], vec![2], vec![3]]);
    }

    #[test]
    fn parse_single_goal() {
        let dag = ExecutionDag::parse("0", 1).unwrap();
        assert_eq!(dag.levels(), &[vec![0]]);
    }

    #[test]
    fn parse_all_parallel() {
        let dag = ExecutionDag::parse("0,1,2", 3).unwrap();
        assert_eq!(dag.levels(), &[vec![0, 1, 2]]);
    }

    #[test]
    fn rejects_out_of_range() {
        let err = ExecutionDag::parse("0,5->2", 4).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn rejects_duplicate() {
        let err = ExecutionDag::parse("0,1->1", 2).unwrap_err();
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn rejects_empty() {
        let err = ExecutionDag::parse("", 1).unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn rejects_incomplete_coverage() {
        let err = ExecutionDag::parse("0->1", 3).unwrap_err();
        assert!(err.to_string().contains("covers 2 indices but expected 3"));
    }

    #[test]
    fn rejects_non_numeric() {
        let err = ExecutionDag::parse("a,b->c", 3).unwrap_err();
        assert!(err.to_string().contains("invalid index"));
    }
}
