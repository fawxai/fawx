# Skill Extraction Plan — Seeding the Marketplace

**Status:** Draft
**Priority:** Pre-OSS launch

## Goal

Extract built-in tools from `fx-tools/FawxToolExecutor` into standalone WASM skill repos under the `fawxai` GitHub organization. This seeds the marketplace on day one and demonstrates the skill authoring pattern for community contributors.

## Current Built-in Tools

All tools currently live in `engine/crates/fx-tools/src/tools.rs` as a monolithic `FawxToolExecutor`. Here's the full inventory:

### Core Tools (keep built-in)
These are tightly coupled to the kernel or engine runtime. They should remain built-in:

| Tool | Why it stays |
|------|-------------|
| `self_info` | Reads internal runtime state (model, skills, config) |
| `current_time` | Trivial, no dependencies |
| `spawn_agent` | Wired to subagent control infrastructure |
| `subagent_status` | Same as above |
| `run_experiment` | Coupled to experiment registry, chain state |
| `emit_intent` | Part of reasoning layer (fx-kernel) |

### Extractable to WASM Skills (marketplace repos)

| Repo | Tools | Description |
|------|-------|-------------|
| `fawxai/skill-filesystem` | `read_file`, `write_file`, `edit_file`, `list_directory`, `search_text` | File system operations |
| `fawxai/skill-shell` | `run_command`, `exec_background`, `exec_status` | Command execution and process management |
| `fawxai/skill-memory` | `memory_read`, `memory_write`, `memory_list`, `memory_delete` | Persistent memory store |
| `fawxai/skill-git` | `git_status`, `git_diff`, `git_commit` | Git operations (already a separate `GitSkill`) |
| `fawxai/skill-config` | `update_config` | Runtime config management |
| `fawxai/skill-node-run` | `node_run` | Remote node command execution |
| `fawxai/skill-journal` | `journal_write`, `journal_read` | Session journal / reflective memory |
| `fawxai/skill-notify` | `notify` | User notifications |
| `fawxai/skill-cron` | `schedule`, `list_schedules`, `cancel_schedule` | Task scheduling |

### Already Extracted (fawxai org)
| Repo | Status |
|------|--------|
| `fawxai/skill-brave-search` | Published |
| `fawxai/skill-web-fetch` | Published |
| `fawxai/skill-scheduler` | Published |

### New Skills to Build (marketplace expansion)
These don't exist yet but are high-value for the marketing/non-technical user base:

| Repo | Description | Priority |
|------|-------------|----------|
| `fawxai/skill-twitter` | Post, schedule, read timeline via Twitter API v2 | High |
| `fawxai/skill-linkedin` | Post updates, read feed | High |
| `fawxai/skill-email` | Send/receive email (IMAP/SMTP or SendGrid) | High |
| `fawxai/skill-calendar` | Google Calendar / Apple Calendar integration | Medium |
| `fawxai/skill-notion` | Read/write Notion pages and databases | Medium |
| `fawxai/skill-slack` | Send messages, read channels | Medium |
| `fawxai/skill-image-gen` | DALL-E / Stable Diffusion image generation | Medium |
| `fawxai/skill-analytics` | Google Analytics, Plausible, etc. | Low |

## Extraction Strategy

### Phase 1: Facade extraction (pre-OSS launch)
Don't rip out the built-in implementations yet. Instead:

1. Create each repo in fawxai org with the WASM skill structure
2. The WASM skill calls the same logic as the built-in tool
3. Built-in tools remain as the default; WASM versions are installable alternatives
4. This populates the marketplace without breaking anything

### Phase 2: True extraction (post-launch)
Once the WASM skill ecosystem is validated:

1. Move tool implementations from `fx-tools` into standalone crates
2. Built-in tools become thin wrappers that load the default skills
3. Users can swap, configure, or disable any skill
4. `FawxToolExecutor` shrinks to just the core kernel tools

### Repo Template Structure
Each skill repo follows the same structure:

```
fawxai/skill-<name>/
├── Cargo.toml
├── LICENSE              (Apache 2.0 for skills)
├── README.md
├── src/
│   └── lib.rs           (implements Skill trait)
├── skill.toml           (metadata: name, version, description, tools)
└── tests/
    └── integration.rs
```

### skill.toml Format
```toml
[skill]
name = "filesystem"
version = "0.1.0"
description = "Read, write, edit, search, and list files and directories"
author = "Fawx AI"
license = "Apache-2.0"

[[tools]]
name = "read_file"
description = "Read a UTF-8 text file from disk"

[[tools]]
name = "write_file"
description = "Write UTF-8 content to a file on disk"
```

## Implementation Order

1. **Create skill repo template** (`fawxai/skill-template`) with cargo-generate support
2. **Extract `skill-filesystem`** — highest visibility, most used tools
3. **Extract `skill-shell`** — second most used
4. **Extract `skill-git`** — already partially separated as `GitSkill`
5. **Extract `skill-memory`** — important for the "personal assistant" story
6. **Build `skill-twitter`** — first "paws and teeth" skill for marketing users
7. **Build `skill-email`** — second marketing-critical skill
8. Remaining extractions and new skills in parallel

## Licensing

- **Engine** (`fawx`): BSL 1.1 (Change License: Apache 2.0, Change Date: 2030-03-23)
- **Skills** (all `fawxai/skill-*` repos): Apache 2.0
- **Skill SDK**: Apache 2.0

Community-authored skills use whatever license the author chooses, but Apache 2.0 is recommended for marketplace inclusion.

## Success Criteria

- At least 10 skills in the fawxai org on launch day
- Each skill has a README with install instructions and usage examples
- `skill-template` works with `cargo generate`
- Marketplace page on fawx.ai shows all available skills
