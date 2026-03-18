# Git Push + PR Tools — Spec

Date: 2026-03-17
Status: Implementation-ready

## Goal

Add `git_push` and `github_pr_create` tools to `GitSkill` so Fawx agents
can push branches and open PRs. These tools use the GitHub token from the
credential provider (which PAT borrowing makes available to subagents).

## Current State

- `GitSkill` has 8 tools (status, diff, checkpoint, branch_create/switch/delete, merge, revert)
- All shell out to `git` via `run_git_with_timeout()`
- No network/remote operations exist
- No credential access — `GitSkill` has no `CredentialProvider` reference
- The `CredentialProvider` trait (in `fx-skills`) maps `"github_token"` to the PAT

## Design

### New constructor parameter

```rust
pub struct GitSkill {
    working_dir: PathBuf,
    self_modify: Option<SelfModifyConfig>,
    credential_provider: Option<Arc<dyn CredentialProvider>>,  // NEW
}
```

Update `GitSkill::new()` to accept the credential provider:
```rust
pub fn new(
    working_dir: PathBuf,
    self_modify: Option<SelfModifyConfig>,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
) -> Self
```

The `CredentialProvider` trait is in `fx-skills::live_host_api`. Add `fx-skills` as
a dependency of `fx-tools` if not already present, or re-export the trait. Check
the dependency graph first — if `fx-skills` depends on `fx-tools`, we can't add
the reverse dep. In that case, define a simpler local trait or just accept an
`Option<Arc<dyn Fn(&str) -> Option<Zeroizing<String>> + Send + Sync>>`.

ACTUALLY: checking the crate graph, `fx-skills` depends on `fx-tools` (through
`fx-loadable` → skill execution). So we can't depend on `fx-skills` from `fx-tools`.

**Solution**: Use a simple closure-based approach:
```rust
/// Provider for GitHub token used in remote operations.
pub type GitHubTokenProvider = Arc<dyn Fn() -> Option<zeroize::Zeroizing<String>> + Send + Sync>;
```

Or even simpler — since we only need one credential (`github_token`):
```rust
pub struct GitSkill {
    working_dir: PathBuf,
    self_modify: Option<SelfModifyConfig>,
    github_token: Option<Arc<dyn Fn() -> Option<zeroize::Zeroizing<String>> + Send + Sync>>,
}
```

Then in `startup.rs`, wire it:
```rust
let github_token_fn: Option<Arc<dyn Fn() -> Option<Zeroizing<String>> + Send + Sync>> =
    credential_provider.clone().map(|cp| {
        Arc::new(move || cp.get_credential("github_token"))
            as Arc<dyn Fn() -> Option<Zeroizing<String>> + Send + Sync>
    });
let git_skill = GitSkill::new(options.working_dir.clone(), sm.clone(), github_token_fn);
```

### Tool 1: `git_push`

**Arguments:**
```json
{
    "remote": "origin",     // optional, defaults to "origin"
    "branch": "feat/foo"    // optional, defaults to current branch
}
```

**Behavior:**
1. Validate `remote` and `branch` (no `-` prefix, no spaces, no `..`)
2. Resolve GitHub token from `self.github_token`
3. If no token: return error "GitHub token not configured. Set up GitHub auth via `fawx setup` or configure a PAT."
4. Create a temporary `GIT_ASKPASS` script that echoes the token
5. Run `git push <remote> <branch>` with env `GIT_ASKPASS=<script_path>` and `GIT_TERMINAL_PROMPT=0`
6. Clean up the temp script (use `tempfile` crate, script deleted on drop)
7. Return stdout/stderr output

**Timeout:** 30 seconds (network operation).

**Security considerations:**
- Token is written to a temp file with `0o700` permissions, deleted immediately after push
- `GIT_TERMINAL_PROMPT=0` prevents interactive prompts
- No token in command args (would show in `ps`)
- Token in env is acceptable (standard git practice)

**GIT_ASKPASS approach:**
The script content is:
```bash
#!/bin/sh
echo "<token>"
```
This is what git calls when it needs a password. For HTTPS GitHub remotes, git asks for a password, and this script provides the PAT.

WAIT — `GIT_ASKPASS` gets called with a prompt argument. For GitHub HTTPS, the flow is:
- Username prompt → script returns the token (GitHub accepts PAT as username with empty password, or as password with any username)

Actually, the simplest approach that's well-tested: use git's `credential.helper` mechanism via environment:

```rust
async fn run_git_with_token(
    &self,
    args: &[&str],
    token: &str,
    timeout_duration: Duration,
) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(&self.working_dir)
        .arg("-c")
        .arg(format!("http.extraheader=Authorization: Bearer {token}"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);
    // ... same as run_git_with_timeout
}
```

This sets the `Authorization: Bearer` header on all HTTP requests for this git invocation. No temp files needed. The token is in the process args but only visible to the same user (and it's a child process that dies immediately).

ACTUALLY — `http.extraheader` with the token shows up in `/proc/PID/cmdline`. The `GIT_ASKPASS` temp-file approach is more secure. But it's also more complex.

**Pragmatic choice**: Use the `-c credential.helper=...` approach with a shell one-liner:

```rust
.arg("-c")
.arg(format!(
    "credential.helper=!f() {{ echo \"password={}\"; }}; f",
    token
))
```

No wait, that's fragile. Let's go with the environment variable approach that git natively supports:

For GitHub HTTPS URLs, the token can be injected via URL rewriting:
```
git -c url."https://x-access-token:{token}@github.com/".insteadOf="https://github.com/" push ...
```

This is the approach used by GitHub Actions and most CI systems. It's clean, no temp files, token is in args but only for the child process lifetime.

**FINAL APPROACH**: Use `url.insteadOf` config override:
```rust
let insteadof_value = format!(
    "url.\"https://x-access-token:{}@github.com/\".insteadOf",
    token
);
// git -c '<insteadof_value>=https://github.com/' push ...
```

This is the most battle-tested approach (GitHub Actions uses it).

### Tool 2: `github_pr_create`

**Arguments:**
```json
{
    "title": "feat: add PAT borrowing",
    "body": "Description of changes...",  // optional
    "base": "dev",                         // optional, defaults to repo default branch
    "head": "feat/pat-borrowing",          // optional, defaults to current branch
    "draft": false                         // optional, defaults to false
}
```

**Behavior:**
1. Validate arguments (title required, non-empty)
2. Get GitHub token
3. Detect remote URL from `git remote get-url origin` → parse owner/repo
4. Call GitHub API: `POST /repos/{owner}/{repo}/pulls`
5. Return: PR URL, number, and status

**Implementation:** Use `reqwest` (already a workspace dep) for the API call.
This is NOT a git operation — it's a GitHub REST API call. So it lives in
`git_skill.rs` but uses HTTP, not git CLI.

**Timeout:** 15 seconds.

### File changes

| File | Change |
|------|--------|
| `engine/crates/fx-tools/src/git_skill.rs` | Add `github_token` field, `git_push` + `github_pr_create` tools, credential helper logic |
| `engine/crates/fx-tools/Cargo.toml` | Add `reqwest`, `tempfile` if needed (check if already deps) |
| `engine/crates/fx-cli/src/startup.rs` | Update `GitSkill::new()` call to pass credential provider closure |

### Tests

**Unit tests (git_skill.rs):**
1. `git_skill_provides_ten_tool_definitions` — update from 8 to 10
2. `git_push_validates_remote_name` — rejects `-evil`, spaces, `..`
3. `git_push_validates_branch_name` — rejects invalid branch names
4. `git_push_requires_github_token` — no token provider → clear error
5. `github_pr_create_requires_title` — empty/missing title → error
6. `github_pr_create_requires_github_token` — no token → clear error
7. `parse_github_remote_extracts_owner_repo` — test HTTPS and SSH URL parsing
8. `git_push_definition_includes_usage_guidance` — description has "Use this when"
9. `github_pr_create_definition_includes_usage_guidance`
10. Existing test `git_skill_provides_eight_tool_definitions` → update to 10
11. Existing test `git_tool_descriptions_include_when_to_use_guidance` → update tool list

**Integration-worthy (manual, Joe tests):**
- Configure PAT, push to a throwaway branch
- Create a PR via the tool, verify on GitHub

### Estimated scope

~300-400 lines including tests. One PR.
