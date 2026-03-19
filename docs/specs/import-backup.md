# Spec: `fawx import` + `fawx backup`

**Issue:** #1301  
**Phase:** 3b (Ops)  
**Status:** Draft  
**Author:** Clawdio  
**Date:** 2026-03-10

---

## `fawx import --from openclaw`

### CLI

```
fawx import --from openclaw [SOURCE_DIR]

Arguments:
  [SOURCE_DIR]    Path to OpenClaw workspace [default: ~/.openclaw/workspace]

Options:
  --dry-run       Show what would be copied without copying
  --force         Overwrite existing files in ~/.fawx/
```

### File Mapping

Clean copy. No parsing, no transformation, **no data loss**.

**Principle: a migration tool never loses data.** Everything gets copied. Recognized files go to their mapped locations. Everything else goes to `~/.fawx/context/`.

#### Recognized files (mapped to specific locations)

| Source (OpenClaw) | Destination (Fawx) | What it is |
|---|---|---|
| `MEMORY.md` | `~/.fawx/memory/MEMORY.md` | Long-term curated memory |
| `memory/*.md` | `~/.fawx/memory/*.md` | Daily memory logs |
| `memory/archive/**/*.md` | `~/.fawx/memory/archive/**/*.md` | Archived memory (preserve structure) |

#### Everything else → context directory

All other `.md` files in the workspace root go to `~/.fawx/context/`:

| Source (OpenClaw) | Destination (Fawx) |
|---|---|
| `SOUL.md` | `~/.fawx/context/SOUL.md` |
| `USER.md` | `~/.fawx/context/USER.md` |
| `AGENTS.md` | `~/.fawx/context/AGENTS.md` |
| `IDENTITY.md` | `~/.fawx/context/IDENTITY.md` |
| `TOOLS.md` | `~/.fawx/context/TOOLS.md` |
| `ENGINEERING.md` | `~/.fawx/context/ENGINEERING.md` |
| `BOOTSTRAP.md` | `~/.fawx/context/BOOTSTRAP.md` |
| `*.md` (any other) | `~/.fawx/context/*.md` |

Fawx loads **all** `.md` files from `~/.fawx/context/` into the system prompt. Users can add, remove, or rename files freely. No hardcoded file list.

### Behavior

1. Validate source directory exists and contains at least one `.md` file
2. Create `~/.fawx/memory/` and `~/.fawx/context/` directories if they don't exist
3. Copy memory files (MEMORY.md, memory/*.md, memory/archive/**) to `~/.fawx/memory/`
4. Copy all root `.md` files (except MEMORY.md) to `~/.fawx/context/`
5. Never overwrite without `--force`. If a destination file exists, print a skip message
6. Print a summary at the end

### Output

```
$ fawx import --from openclaw ~/.openclaw/workspace

🦊 Importing from OpenClaw

  Memory:
    ✓ MEMORY.md            → ~/.fawx/memory/MEMORY.md
    ✓ memory/2026-03-09.md → ~/.fawx/memory/2026-03-09.md
    ✓ memory/2026-03-10.md → ~/.fawx/memory/2026-03-10.md

  Context:
    ✓ SOUL.md              → ~/.fawx/context/SOUL.md
    ✓ USER.md              → ~/.fawx/context/USER.md
    ✓ AGENTS.md            → ~/.fawx/context/AGENTS.md
    ✓ IDENTITY.md          → ~/.fawx/context/IDENTITY.md
    ✓ TOOLS.md             → ~/.fawx/context/TOOLS.md
    ✓ ENGINEERING.md       → ~/.fawx/context/ENGINEERING.md
    ✓ BOOTSTRAP.md         → ~/.fawx/context/BOOTSTRAP.md

  Imported 10 files (3 memory, 7 context). Your memory and context are ready.
  Fawx loads all .md files from ~/.fawx/context/ automatically.
```

### Dry run output

```
$ fawx import --from openclaw --dry-run

🦊 Import preview (dry run)

  Memory:
    MEMORY.md            → ~/.fawx/memory/MEMORY.md
    memory/2026-03-09.md → ~/.fawx/memory/2026-03-09.md

  Context:
    SOUL.md              → ~/.fawx/context/SOUL.md
    ...

  Would import 10 files. Run without --dry-run to proceed.
```

### Skip output (existing files)

```
  ⊘ MEMORY.md          — already exists (use --force to overwrite)
```

### Error handling

| Scenario | Behavior |
|---|---|
| Source dir doesn't exist | `"Directory not found: ~/.openclaw/workspace"` |
| Source dir has no .md files | `"No markdown files found in <path>. Is this an OpenClaw workspace?"` |
| Destination file exists (no --force) | Skip with message, continue others |
| Permission error on copy | Print error for that file, continue others |
| ~/.fawx doesn't exist | Create it |

---

## `fawx backup`

### CLI

```
fawx backup [OPTIONS]

Options:
  --output <DIR>    Output directory [default: ~/.fawx/backups/]
```

### Behavior

1. Tar + gzip the entire `~/.fawx/` directory (excluding `backups/` subdirectory)
2. Output file: `fawx-backup-YYYY-MM-DD-HHMMSS.tar.gz`
3. Print the path and size

### What gets backed up

- `config.toml` (configuration)
- `auth.db` (encrypted credentials)
- `memory/` (all memory files)
- `context/` (imported context files)
- `skills/` (installed WASM skills + manifests)
- `sessions/` (conversation history)
- `audit.log` (audit trail)
- Excludes: `backups/` directory, `fawx.pid`

### Output

```
$ fawx backup

🦊 Backing up ~/.fawx/

  Config:     config.toml
  Credentials: auth.db (encrypted)
  Memory:     12 files
  Context:    4 files
  Skills:     8 skills
  Sessions:   3 sessions

  ✓ Backup saved: ~/.fawx/backups/fawx-backup-2026-03-10-045200.tar.gz (2.4 MB)
```

### Error handling

| Scenario | Behavior |
|---|---|
| ~/.fawx doesn't exist | `"No Fawx data directory found. Nothing to back up."` |
| Output dir not writable | `"Cannot write to <path>: permission denied"` |
| Tar creation fails | Print error, exit 1 |

---

## Implementation

### Files

- `engine/crates/fx-cli/src/commands/import.rs` — import command
- `engine/crates/fx-cli/src/commands/backup.rs` — backup command
- Register both in `main.rs` Commands enum

### Dependencies

- `flate2` for gzip compression (already in dependency tree via other crates, check first)
- `tar` crate for archive creation (check if already available)
- If neither is available, shell out to system `tar` (always available on Linux/macOS)

### Testing

Import:
1. Import from directory with all files present (recognized + unrecognized .md)
2. Import from directory with some files missing (skips gracefully)
3. Import with existing destination files (skips without --force)
4. Import with --force overwrites existing files
5. Import with --dry-run copies nothing
6. Import from nonexistent directory fails cleanly
7. Import from empty directory fails cleanly
8. Unrecognized .md files copied to context/ (no data loss)
9. memory/archive/ structure preserved recursively

Backup:
1. Backup creates valid tar.gz
2. Backup excludes backups/ directory
3. Backup excludes fawx.pid
4. Backup with custom --output directory
5. Backup with no data directory fails cleanly
