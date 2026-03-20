# Design Doc: Workspace Identity and Self-Reference

**Status:** DRAFT
**Author:** Clawdio (with Joe)
**Date:** 2026-03-19
**Context:** Fresh-install user experience gap discovered during v1.0.0 launch prep

---

## Problem Statement

Fawx's git tools, memory, and backlog features were built for a developer who already has a project open. A fresh user who installs the DMG and starts chatting has no working directory, no repo, and no project context. Several common interactions break or produce confusing results:

1. **"Fix your experiment tooling"** — The user is talking about Fawx itself, but Fawx is a compiled binary with no source code on their machine. There's nothing to fix.

2. **"Commit this and open a PR"** — Git tools operate on cwd, which for a fresh install is either `/` or the user's home directory. No repo exists.

3. **"Add this to the backlog"** — Currently writes to memory files in whatever workspace is active. For a new user, there's no meaningful backlog target.

4. **"Open my project at ~/code/my-app"** — This should work but there's no explicit "project binding" concept. The tools just use cwd.

---

## Design Goals

1. Git tools work naturally when a project is active, fail gracefully with helpful guidance when no project exists.
2. Fawx understands the difference between "fix something in my project" and "fix something in Fawx itself."
3. Memory and notes are personal (tied to the user), not accidentally tied to a project repo.
4. The experience scales from "no project, just chatting" to "multiple repos, switching between them."

---

## Proposed Model

### 1. Explicit Workspace Binding

Fawx maintains a **session-level workspace** concept: the active project directory.

**How it gets set:**
- User says "open ~/code/my-app" or "work on my-app" — Fawx sets the workspace
- User runs Fawx from a directory containing a git repo — auto-detected on startup
- User drags a folder onto the Fawx window (macOS) — sets workspace
- No project mentioned — workspace is unset, git tools are unavailable

**What it affects:**
- All git operations (commit, branch, PR, diff) scope to the active workspace
- File read/write tools scope to the workspace root
- Project-specific context (README, structure, language) is loaded from the workspace

**What it does NOT affect:**
- Memory and notes (always in `~/.fawx/memory/`)
- Preferences and config (always in `~/.fawx/`)
- Conversations (stored per-session in `~/.fawx/`)

### 2. Self-Reference Boundary

Fawx needs a clear internal model for "requests about me vs. requests about the user's project," AND a clear model for what it can and can't change about itself.

**The loadable/kernel distinction matters here.** Fawx has two layers:
- **Loadable layer** (skills, config, system prompt, memory, WASM plugins): Fawx CAN modify these. A user saying "change how you search the web" or "add a new skill" is actionable.
- **Kernel layer** (compiled binary, core loop, safety enforcement, built-in tool implementations): Fawx CANNOT modify these at runtime. A user saying "fix your experiment tooling" is asking for a kernel change.

**Detection heuristics:**
- References to "Fawx", "your code", "your tools", "your skills", "this app" → self-reference
- References to "my project", "this repo", "the code", file paths → project reference
- Ambiguous → ask: "Are you asking me to change something in your project, or asking about changing how I work?"

**Self-reference responses by layer:**

| Request type | Layer | Response |
|-------------|-------|----------|
| "Change your system prompt" | Loadable | Actionable. Fawx can edit its own synthesis/custom instructions. |
| "Add a skill for X" | Loadable | Actionable. Fawx can install WASM skills from the marketplace or help build one. |
| "Adjust your permissions" | Loadable | Actionable. Config change via setup wizard or settings. |
| "Fix your experiment tooling" | Kernel | Not actionable locally. Route to feedback channel. |
| "Your streaming is broken" | Kernel | Not actionable locally. Route to feedback channel. |
| "Change how you handle tool errors" | Kernel | Not actionable locally. Route to feedback channel. |

**Feedback channel for kernel issues:**
- `abbudjoe/fawx` is a private repo; external users cannot file issues there
- Primary channel: community Discord / feedback form on fawx.ai
- Fawx should say: "That's part of my core engine, which I can't modify myself. You can report it at fawx.ai/feedback [or Discord link] and the team will see it."
- Fawx can also log the feedback locally in `~/.fawx/feedback/` so the user has a record, and optionally submit it via an API endpoint if one exists

**Implementation:** This is primarily a system prompt / tool-routing concern, not a code architecture change. The system prompt should include:
- Awareness of whether Fawx is running in "development mode" (source checkout) vs. "installed mode" (DMG install)
- A clear description of the loadable vs. kernel boundary
- Guidance on how to route kernel feedback to the right place

### 3. Git Tool Graceful Degradation

When no workspace is active, git tools should return helpful guidance instead of errors.

**Current behavior:** Tool calls against no repo produce cryptic git errors or operate on the wrong directory.

**Proposed behavior:**

| Scenario | Response |
|----------|----------|
| No workspace set, user says "commit this" | "I don't have a project open right now. Tell me which directory to work in, like 'open ~/code/my-app'." |
| Workspace set but not a git repo | "~/code/my-app isn't a git repository. Want me to initialize one with `git init`?" |
| Workspace set, repo exists | Normal git operations |
| User says "open" + valid path | Set workspace, confirm: "Working in ~/code/my-app (git branch: main, 3 modified files)." |

### 4. Memory Separation

**Personal memory** (preferences, notes, backlog items the user dictates):
- Lives in `~/.fawx/memory/`
- Persists across projects and sessions
- "Remember that I prefer tabs over spaces" → personal memory
- "Add to my backlog: refactor the auth module" → personal memory (project-tagged if workspace is active)

**Project context** (code structure, READMEs, dependency info):
- Read from the active workspace on demand
- Not persisted in `~/.fawx/` (it's already in the repo)
- Refreshed when workspace changes

**Backlog specifically:**
- When a workspace is active and it has a GitHub remote, "add to backlog" could mean "create a GitHub issue"
- When no workspace or no remote, "add to backlog" writes to personal memory with a tag
- Ask on ambiguity: "Want me to create a GitHub issue for this, or just note it in your personal backlog?"

---

## UX Flows

### Fresh install, no project

```
User: Hey Fawx, what can you do?
Fawx: I'm an AI agent that runs locally on your Mac. I can help with coding,
      research, writing, and automation. To work on a project, tell me
      where it lives: "open ~/code/my-app". Or just chat — I'm useful
      without a project too.

User: Fix your experiment tooling to handle Python projects
Fawx: That's part of my core engine, which I can't modify on my own. I can
      note this as feedback and you can submit it at fawx.ai/feedback so the
      team sees it. In the meantime, I can help you set up a custom experiment
      workflow using my skills layer — want me to try that approach?
```

### Fresh install, user opens a project

```
User: Open ~/projects/my-api
Fawx: Working in ~/projects/my-api. It's a Node.js project (package.json
      found) on branch `main` with 2 uncommitted changes. What do you
      want to work on?

User: Fix the auth middleware bug
Fawx: [reads code, proposes fix, can commit/branch/PR normally]
```

### Switching projects

```
User: Switch to ~/projects/frontend
Fawx: Switched to ~/projects/frontend. React app on branch `feature/nav`,
      clean working tree. What's up?
```

---

## Implementation Phases

### Phase 1: Graceful degradation (low effort, high impact)
- Git tools check for active workspace before operating
- Clear error messages when no workspace exists
- "Open [path]" sets the workspace for the session

### Phase 2: Self-reference awareness
- System prompt update for installed-mode vs. development-mode
- Loadable vs. kernel routing: what Fawx can change about itself vs. what needs feedback
- Detection heuristics for "about Fawx" vs. "about my project"
- Local feedback logging (`~/.fawx/feedback/`) + optional submission API
- First-run documentation: "What Fawx can do" guide covering loadable customization and kernel boundaries

### Phase 3: Rich workspace binding
- Auto-detect repo on startup if launched from a project directory
- Drag-and-drop folder support (macOS)
- Multi-workspace support ("switch to [project]")
- Project-tagged backlog items

### Phase 4: Project memory
- Per-project preferences and context (coding style, conventions, common patterns)
- Stored in `~/.fawx/projects/{project-hash}/` to avoid polluting repos
- Loaded automatically when workspace is activated

---

## Open Questions

1. **Should Fawx create a `.fawx/` directory inside the user's project?** Probably not for v1 — keep all Fawx state in `~/.fawx/`. Users don't want mystery directories in their repos.

2. **How does this interact with fleet mode?** Fleet workers already have workspace binding through task assignments. The workspace concept here is for the interactive single-user experience.

3. **Should "open" persist across sessions?** Probably yes — if I opened `~/code/my-app` yesterday, Fawx should offer to reopen it today. Store last-used workspace in preferences.

4. **What about multiple simultaneous projects?** Defer to Phase 3. For v1, one active workspace per session is sufficient.

---

## Relationship to Existing Architecture

- **SkillRegistry**: Git tools are skills. Workspace binding is a context they receive, not something they manage internally.
- **fx-journal**: Memory writes already go to `~/.fawx/`. No change needed.
- **System prompt**: Needs a conditional section for installed-mode that includes self-reference handling.
- **Setup wizard**: Could optionally ask "Do you have a project you want to work on?" but this feels premature for first launch. Let users discover it organically.
