# Step 14.1: Session Archive Metadata

## Branch
`codex/step14-1-session-archive-metadata`

## Goal
Teach `fx-session` what an archived session is without changing list behavior, routes, or UI yet.

## Why this slice exists
Everything else in Step 14 depends on a stable session-state contract. The backend cannot archive, filter, or export archived sessions until the persisted session types can represent that state cleanly.

This slice should only add the metadata and helpers needed for later slices.

## Expected targets
- `engine/crates/fx-session/src/types.rs`
- small adjacent serialization or helper modules if required

## Required design
Model archive as reversible metadata.

Preferred shape:
- `archived_at: Option<u64>` on the persisted session metadata or info type
- helper like `is_archived() -> bool`

Why `Option<u64>` instead of a bare `bool`:
- preserves whether the session is archived
- records when the transition happened
- keeps future audit or sort behavior available without another schema change

## Rules
- additive schema change only
- keep backward compatibility with existing stored sessions
- old sessions without `archived_at` must deserialize as active
- do not change delete or clear behavior in this slice
- do not add routes in this slice
- do not move sessions into a new storage location

## Acceptance criteria
- session metadata can represent active and archived states
- existing active sessions deserialize cleanly with `archived_at = None`
- helper methods make archive checks explicit instead of scattering `Option` matching everywhere
- no list, router, or UI behavior changes land yet

## Tests required
1. legacy active session deserializes with no archive timestamp
2. archived session metadata round-trips through serialization
3. `is_archived()` reports false for `None` and true for `Some(...)`
4. archive metadata is preserved through save/load paths used by the session store

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

## Done means
- `fx-session` can represent archived sessions durably
- persistence remains backward compatible
- later slices can build archive behavior without reworking the type contract

## Reviewer focus
- Is archive modeled as durable metadata rather than a special case or second store?
- Is the schema change additive and backward compatible?
- Did this slice avoid leaking behavior changes into routes or list semantics?
