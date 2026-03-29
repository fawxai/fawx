# Spec: Keep Deterministic Workspace Search on a Bounded Read Path

## Problem

A simple exact-phrase workspace search is being routed through broad agentic planning
instead of a bounded read-only path.

On current `main`, a request to search the workspace for one exact phrase, read the
matching file, and return only the first line can fan out into dozens of tool calls,
cross repository boundaries, invoke `python_run`, and still time out or return an
incorrect blocked answer.

This is a control-plane bug. Deterministic read-only requests should not require broad
planning to complete.

## Confirmed Evidence

Observed on 2026-03-29 on a fresh clean lane built from `main` commit `ed81bf7b`.

Session: `sess-477fd738`

User prompt:

`Search the workspace for the exact phrase "Everything Describes Itself", then read the matching file and return only the first line exactly.`

Stored session summary:

- 85 messages total
- 68 tool uses
- 42 `run_command`
- 8 `list_directory`
- 4 `search_text`
- 4 `python_run`
- 4 `kernel_manifest`
- 4 `config_get`
- 1 `exec_background`
- 1 `exec_status`

Observed behavior:

- initial read-only search failed and immediately broadened into repeated shell search
- the agent launched a background `rg` over `/Users/joseph`
- the agent invoked `python_run` for a simple exact-phrase lookup
- repeated observation-only retries eventually triggered no-progress guards
- the final answer claimed it could only search `/Users/joseph/fawx` after already
  searching far outside that boundary

## Expected Behavior

For a deterministic prompt like exact-phrase search plus "return the first line exactly":

1. choose one bounded read-only search path inside the active workspace
2. use direct tools such as `search_text`, `run_command` with `rg`, and `read_file`
3. return the first line on success, or a clean no-match result on failure
4. do not invoke `python_run`, `exec_background`, or unrelated introspection tools
   unless the direct path is genuinely unavailable

## Contract

Deterministic workspace-search requests must use deterministic bounded execution paths.

If the active workspace root is missing or ambiguous:

- resolve it once before planning continues, or
- fail clearly without broadening to the entire home directory

The kernel must not turn a narrow read-only request into open-ended exploratory search.

## Root Cause Definition

The control plane is not recognizing that this request is a narrow utility operation with
an obvious direct execution path.

Instead of selecting a bounded search primitive, the system falls back into the general
planning loop, where retries, discovery steps, and new tool families can compound into
large fan-out.

That breaks two important contracts:

- deterministic requests do not stay deterministic
- read-only search can escalate into unrelated execution primitives

## Fix Scope

### 1. Add a bounded path for deterministic workspace search

When the request is "find exact text in the workspace and read the match", route it
through a direct search-and-read path instead of the broad planning loop.

### 2. Prevent inappropriate escalation for read-only search

For this class of request, do not escalate to `python_run`, `exec_background`, or other
heavyweight execution primitives unless there is a documented hard dependency.

### 3. Keep workspace scope explicit

The effective search root must remain the active workspace, not silently expand to the
user's home directory or sibling repositories.

### 4. Trip the read-only loop breaker earlier

If repeated observation-only attempts are making no progress, terminate with current
findings instead of expanding the tool surface.

## Regression Tests

1. Exact-phrase hit:
   a repository contains one matching file, and the request completes with a bounded
   search/read sequence and no `python_run`.

2. Exact-phrase miss:
   a repository contains no match, and the request returns a clean no-match answer
   without broad fan-out or timeout.

3. Ambiguous workspace:
   if no workspace root is available, the system fails clearly without scanning the
   entire home directory.

4. No-progress guard:
   repeated read-only retries stop before introducing new exploratory tool families.

## Likely Files

- `engine/crates/fx-kernel/src/loop_engine.rs`
- `engine/crates/fx-kernel/src/loop_engine/direct_utility.rs`
- `engine/crates/fx-kernel/src/scoped_tool_executor.rs`
- `engine/crates/fx-tools/src/tools.rs`

## Non-goals

- hiding the failure in SwiftUI
- teaching the model a prose workaround instead of enforcing the contract
- treating this as part of the `#1654` tool-registry refactor
