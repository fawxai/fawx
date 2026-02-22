# Spec Contract Tests

This directory contains the H2.3 spec contract checker, machine-readable contract manifest, and shell test harness.

## Local Run

Run the contract checker against the real spec and constitution:

```bash
./scripts/spec-tests/h23-tool-grouping-spec-contract.sh
```

Run the fixture-driven harness:

```bash
./scripts/spec-tests/tests/h23-tool-grouping-spec-contract-test.sh
./scripts/spec-tests/tests/ci-workflow-cache-paths-test.sh
```

Run both with shellcheck:

```bash
shellcheck scripts/spec-tests/ci-workflow-cache-paths-check.sh \
  scripts/spec-tests/h23-tool-grouping-spec-contract.sh \
  scripts/spec-tests/fixtures/h23-contract-fixture-builder.sh \
  scripts/spec-tests/tests/h23-tool-grouping-spec-contract-test.sh \
  scripts/spec-tests/tests/ci-workflow-cache-paths-test.sh
./scripts/spec-tests/ci-workflow-cache-paths-check.sh
./scripts/spec-tests/h23-tool-grouping-spec-contract.sh
./scripts/spec-tests/tests/h23-tool-grouping-spec-contract-test.sh
./scripts/spec-tests/tests/ci-workflow-cache-paths-test.sh
```

## Shellcheck Version Strategy

- CI installs shellcheck from the current `ubuntu-latest` apt repository to match GitHub-hosted runner defaults.
- Treat CI as the source of truth for lint outcomes; local mismatches should be resolved by aligning local shellcheck to CI output.
- When CI shellcheck behavior changes due to runner image updates, update this README and any affected scripts in the same PR to keep drift explicit.

## Debugging

Run with an explicit fixture file:

```bash
FIXTURES_DIR="$(mktemp -d)"
# shellcheck source=/dev/null
source scripts/spec-tests/fixtures/h23-contract-fixture-builder.sh
build_h23_contract_fixtures "$FIXTURES_DIR"
SPEC_FILE="$FIXTURES_DIR/h23-contract-pass.md" \
CONSTITUTION_FILE="$FIXTURES_DIR/constitution-pass.md" \
./scripts/spec-tests/h23-tool-grouping-spec-contract.sh
```

Run with `bash -x` for trace output:

```bash
bash -x ./scripts/spec-tests/h23-tool-grouping-spec-contract.sh
```

## Expected Failure Examples

- Wrong heading level in constitution (`### 5.2 ...`) fails:
  `expected heading missing in file '.../constitution-fail-wrong-heading-level.md': ## 5.2`
- Inline text that mentions a section token but is not a heading fails:
  `expected heading missing in file '.../constitution-fail-inline-false-positive.md': ## 8.6`
- Missing acceptance text in section `7.3` fails:
  `expected pattern missing in section '7.3 Acceptance criteria' of file '...': Policy-violation counter`

## Failure Taxonomy

| Rule ID | Contract Rule | Example Failure Message Prefix |
|---|---|---|
| `H23-SPEC-001` | Required fixed-string spec tokens from manifest are present | `expected pattern missing in file '...':` |
| `H23-SPEC-002` | Required semantic regex spec patterns from manifest are present | `expected regex missing in file '...':` |
| `H23-SPEC-003` | Forbidden legacy tokens are absent from spec | `unexpected pattern present in file '...':` |
| `H23-SPEC-004` | Required spec headings exist at expected heading levels | `expected heading missing in file '...':` |
| `H23-SPEC-005` | Section-scoped acceptance/examples patterns exist in the target section | `expected pattern missing in section '...' of file '...':` |
| `H23-SPEC-006` | JSON examples do not contradict disable reason-codes | `inconsistent JSON example semantics in 7.4 ResolvedToolPlan examples (normative)` |
| `H23-CI-CACHE-001` | `path:` entries under `uses: actions/cache@...` steps must be user-writable and must not target protected absolute system directories (single-line, block scalar, block list, or inline bracket-list forms) | `[H23-CI-CACHE-001] disallowed cache path detected in ...` |
| `H23-CONST-001` | Constitution required section headings exist at level-2 | `expected heading missing in file '...': ## <section>` |
| `H23-CONST-002` | Constitution section semantics are enforced by section-scoped regex checks (`5.2`, `5.3`, `7.3`, `8.1`, `8.6`, `9`) | `expected regex missing in section '...' of file '...':` |
