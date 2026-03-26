#!/usr/bin/env python3
from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from fnmatch import fnmatchcase
from pathlib import Path
from typing import Sequence

HUNK_HEADER_RE = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")
AUTHOR_FIELD_RE = re.compile(r"\bauthors?\b\s*=")


class GuardError(RuntimeError):
    """Raised when the guard cannot evaluate the repository safely."""


@dataclass(frozen=True)
class NamedPattern:
    name: str
    pattern: re.Pattern[str]


@dataclass(frozen=True)
class AddedLine:
    path: str
    line_number: int
    text: str


@dataclass(frozen=True)
class Finding:
    path: str
    message: str
    line_number: int | None = None
    excerpt: str | None = None


@dataclass(frozen=True)
class GuardConfig:
    base_ref: str
    allowlist: tuple[str, ...]
    blocklist: tuple[str, ...]
    marker_patterns: tuple[NamedPattern, ...]
    author_private_patterns: tuple[re.Pattern[str], ...]
    workflow_private_patterns: tuple[NamedPattern, ...]
    review_warning_file_count: int
    review_warning_area_count: int


@dataclass(frozen=True)
class DiffContext:
    base_ref: str
    changed_paths: tuple[str, ...]
    active_paths: tuple[str, ...]
    added_lines: tuple[AddedLine, ...]


@dataclass(frozen=True)
class Report:
    context: DiffContext
    blocked_paths: tuple[str, ...]
    allowlist_misses: tuple[str, ...]
    marker_findings: tuple[Finding, ...]
    invariant_findings: tuple[Finding, ...]
    warnings: tuple[str, ...]

    @property
    def failed(self) -> bool:
        return any(
            [
                self.blocked_paths,
                self.allowlist_misses,
                self.marker_findings,
                self.invariant_findings,
            ]
        )


def main() -> int:
    try:
        report = build_report(Path(__file__).resolve().parent)
    except GuardError as error:
        print("check-public-promotion: FAIL")
        print()
        print(error)
        return 1

    print(render_report(report))
    return 1 if report.failed else 0


def build_report(script_dir: Path) -> Report:
    config = load_config(script_dir / "check-public-promotion.toml")
    repo_root = resolve_repo_root(script_dir)
    context = collect_diff_context(repo_root, config.base_ref)
    blocked_paths = find_matching_paths(context.changed_paths, config.blocklist)
    allowlist_misses = find_allowlist_misses(
        context.changed_paths,
        config.allowlist,
        blocked_paths,
    )
    marker_findings = scan_added_lines(context.added_lines, config.marker_patterns)
    invariant_findings = collect_invariant_findings(repo_root, context, config)
    warnings = build_warnings(context.changed_paths, config)
    return Report(
        context=context,
        blocked_paths=blocked_paths,
        allowlist_misses=allowlist_misses,
        marker_findings=marker_findings,
        invariant_findings=invariant_findings,
        warnings=warnings,
    )


def load_config(config_path: Path) -> GuardConfig:
    with config_path.open("rb") as handle:
        raw = tomllib.load(handle)

    return GuardConfig(
        base_ref=raw["base_ref"],
        allowlist=tuple(raw["allowlist"]),
        blocklist=tuple(raw["blocklist"]),
        marker_patterns=compile_named_patterns(raw["markers"]),
        author_private_patterns=compile_patterns(raw["author_private_patterns"]),
        workflow_private_patterns=compile_named_patterns(raw["workflow_private_markers"]),
        review_warning_file_count=int(raw["review_warning_file_count"]),
        review_warning_area_count=int(raw["review_warning_area_count"]),
    )


def compile_named_patterns(raw_patterns: Sequence[dict[str, str]]) -> tuple[NamedPattern, ...]:
    compiled = []
    for entry in raw_patterns:
        compiled.append(NamedPattern(entry["name"], re.compile(entry["pattern"])))
    return tuple(compiled)


def compile_patterns(raw_patterns: Sequence[str]) -> tuple[re.Pattern[str], ...]:
    return tuple(re.compile(pattern) for pattern in raw_patterns)


def resolve_repo_root(script_dir: Path) -> Path:
    output = git_stdout(script_dir, ["rev-parse", "--show-toplevel"])
    return Path(output.strip())


def collect_diff_context(repo_root: Path, base_ref: str) -> DiffContext:
    ensure_ref_exists(repo_root, base_ref)
    diff_range = f"{base_ref}...HEAD"
    changed_paths = tuple(git_lines(repo_root, ["diff", "--name-only", diff_range]))
    active_paths = tuple(
        git_lines(repo_root, ["diff", "--name-only", "--diff-filter=ACMR", diff_range])
    )
    added_lines = tuple(collect_added_lines(repo_root, diff_range, active_paths))
    return DiffContext(base_ref, changed_paths, active_paths, added_lines)


def ensure_ref_exists(repo_root: Path, ref_name: str) -> None:
    result = subprocess.run(
        ["git", "rev-parse", "--verify", f"{ref_name}^{{commit}}"],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode == 0:
        return
    raise GuardError(
        f"Base ref '{ref_name}' is missing. Fetch it first, for example: git fetch public main"
    )


def collect_added_lines(
    repo_root: Path,
    diff_range: str,
    active_paths: Sequence[str],
) -> list[AddedLine]:
    if not active_paths:
        return []
    diff_text = git_stdout(
        repo_root,
        ["diff", "--unified=0", "--no-color", diff_range, "--", *active_paths],
    )
    return parse_added_lines(diff_text, set(active_paths))


def parse_added_lines(diff_text: str, active_paths: set[str]) -> list[AddedLine]:
    findings: list[AddedLine] = []
    current_path: str | None = None
    line_number: int | None = None
    for raw_line in diff_text.splitlines():
        if raw_line.startswith("diff --git "):
            current_path = None
            line_number = None
            continue
        path = parse_diff_path(raw_line, active_paths)
        if path is not None:
            current_path = path
            line_number = None
            continue
        header = HUNK_HEADER_RE.match(raw_line)
        if header:
            line_number = int(header.group(1))
            continue
        if raw_line == r"\ No newline at end of file":
            continue
        if current_path is None or line_number is None:
            continue
        if raw_line.startswith("+"):
            findings.append(AddedLine(current_path, line_number, raw_line[1:]))
            line_number += 1
            continue
        if raw_line.startswith("-"):
            continue
        line_number += 1
    return findings


def parse_diff_path(raw_line: str, active_paths: set[str]) -> str | None:
    if not raw_line.startswith("+++ "):
        return None
    path = strip_diff_prefix(raw_line[4:])
    if path == "/dev/null" or path not in active_paths:
        return None
    return path


def strip_diff_prefix(path: str) -> str:
    if path.startswith(("a/", "b/")):
        return path[2:]
    return path


def git_stdout(repo_root: Path, args: Sequence[str]) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode == 0:
        return result.stdout
    message = result.stderr.strip() or result.stdout.strip() or "git command failed"
    raise GuardError(message)


def git_lines(repo_root: Path, args: Sequence[str]) -> list[str]:
    return [line for line in git_stdout(repo_root, args).splitlines() if line.strip()]


def find_matching_paths(paths: Sequence[str], patterns: Sequence[str]) -> tuple[str, ...]:
    return tuple(path for path in paths if any(path_matches(path, pattern) for pattern in patterns))


def find_allowlist_misses(
    paths: Sequence[str],
    allowlist: Sequence[str],
    blocked_paths: Sequence[str],
) -> tuple[str, ...]:
    blocked_set = set(blocked_paths)
    misses = []
    for path in paths:
        if path in blocked_set:
            continue
        if any(path_matches(path, pattern) for pattern in allowlist):
            continue
        misses.append(path)
    return tuple(misses)


def path_matches(path: str, pattern: str) -> bool:
    if fnmatchcase(path, pattern):
        return True
    return pattern.endswith("/**") and path == pattern[:-3]


def scan_added_lines(
    added_lines: Sequence[AddedLine],
    patterns: Sequence[NamedPattern],
) -> tuple[Finding, ...]:
    findings: list[Finding] = []
    for added_line in added_lines:
        match = first_named_match(added_line.text, patterns)
        if match is None:
            continue
        findings.append(
            Finding(
                path=added_line.path,
                line_number=added_line.line_number,
                message=match.name,
                excerpt=added_line.text.strip(),
            )
        )
    return tuple(findings)


def first_named_match(text: str, patterns: Sequence[NamedPattern]) -> NamedPattern | None:
    for pattern in patterns:
        if pattern.pattern.search(text):
            return pattern
    return None


def collect_invariant_findings(
    repo_root: Path,
    context: DiffContext,
    config: GuardConfig,
) -> tuple[Finding, ...]:
    findings: list[Finding] = []
    findings.extend(check_llama_reintroduction(repo_root, context))
    findings.extend(check_author_metadata(context.added_lines, config))
    findings.extend(check_workflow_refs(context.added_lines, config))
    return tuple(findings)


def check_llama_reintroduction(
    repo_root: Path,
    context: DiffContext,
) -> tuple[Finding, ...]:
    if ref_has_path(repo_root, context.base_ref, "engine/crates/llama-cpp-sys/Cargo.toml"):
        return ()

    findings = []
    for added_line in context.added_lines:
        if "llama-cpp-sys" not in added_line.text:
            continue
        findings.append(
            Finding(
                path=added_line.path,
                line_number=added_line.line_number,
                message="llama-cpp-sys is absent from the base ref and should not be reintroduced",
                excerpt=added_line.text.strip(),
            )
        )
    return tuple(findings)


def ref_has_path(repo_root: Path, ref_name: str, repo_path: str) -> bool:
    result = subprocess.run(
        ["git", "cat-file", "-e", f"{ref_name}:{repo_path}"],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    return result.returncode == 0


def check_author_metadata(
    added_lines: Sequence[AddedLine],
    config: GuardConfig,
) -> tuple[Finding, ...]:
    findings = []
    for added_line in added_lines:
        if not is_author_metadata_file(added_line.path):
            continue
        if not AUTHOR_FIELD_RE.search(added_line.text):
            continue
        if not any(pattern.search(added_line.text) for pattern in config.author_private_patterns):
            continue
        findings.append(
            Finding(
                path=added_line.path,
                line_number=added_line.line_number,
                message="author metadata should stay public-safe (for example: Fawx AI)",
                excerpt=added_line.text.strip(),
            )
        )
    return tuple(findings)


def is_author_metadata_file(path: str) -> bool:
    return path.endswith("Cargo.toml") or path.endswith("manifest.toml")


def check_workflow_refs(
    added_lines: Sequence[AddedLine],
    config: GuardConfig,
) -> tuple[Finding, ...]:
    findings = []
    for added_line in added_lines:
        if not added_line.path.startswith(".github/workflows/"):
            continue
        match = first_named_match(added_line.text, config.workflow_private_patterns)
        if match is None:
            continue
        findings.append(
            Finding(
                path=added_line.path,
                line_number=added_line.line_number,
                message=f"public workflow references {match.name}",
                excerpt=added_line.text.strip(),
            )
        )
    return tuple(findings)


def build_warnings(paths: Sequence[str], config: GuardConfig) -> tuple[str, ...]:
    areas = top_level_areas(paths)
    is_broad = len(paths) > config.review_warning_file_count
    is_wide = len(areas) > config.review_warning_area_count
    if not is_broad and not is_wide:
        return ()
    warning = (
        f"{len(paths)} changed files across {len(areas)} top-level areas; "
        "confirm this promotion is intentionally scoped."
    )
    return (warning,)


def top_level_areas(paths: Sequence[str]) -> tuple[str, ...]:
    areas = set()
    for path in paths:
        parts = Path(path).parts
        areas.add(parts[0] if parts else path)
    return tuple(sorted(areas))


def render_report(report: Report) -> str:
    lines = [
        f"check-public-promotion: {'FAIL' if report.failed else 'PASS'}",
        "",
        f"Base ref: {report.context.base_ref}",
        f"Changed files: {len(report.context.changed_paths)}",
    ]
    append_path_section(lines, "Blocked paths", report.blocked_paths)
    append_path_section(lines, "Allowlist misses", report.allowlist_misses)
    append_finding_section(lines, "Private markers", report.marker_findings)
    append_finding_section(lines, "Public invariants", report.invariant_findings)
    append_warning_section(lines, report.warnings)
    if report.failed:
        append_suggested_actions(lines)
    return "\n".join(lines)


def append_path_section(lines: list[str], title: str, entries: Sequence[str]) -> None:
    if not entries:
        return
    lines.extend(["", f"{title}:"])
    lines.extend(f"- {entry}" for entry in entries)


def append_finding_section(lines: list[str], title: str, findings: Sequence[Finding]) -> None:
    if not findings:
        return
    lines.extend(["", f"{title}:"])
    lines.extend(f"- {format_finding(finding)}" for finding in findings)


def append_warning_section(lines: list[str], warnings: Sequence[str]) -> None:
    if not warnings:
        return
    lines.extend(["", "Warnings:"])
    lines.extend(f"- {warning}" for warning in warnings)


def append_suggested_actions(lines: list[str]) -> None:
    lines.extend(
        [
            "",
            "Suggested action:",
            "- split mixed commits into a narrower promotion branch",
            "- remove blocked or non-allowlisted files from the promotion diff",
            "- scrub private markers or private metadata and rerun the guard",
        ]
    )


def format_finding(finding: Finding) -> str:
    location = finding.path
    if finding.line_number is not None:
        location = f"{location}:{finding.line_number}"
    if finding.excerpt:
        return f"{location} [{finding.message}] {finding.excerpt}"
    return f"{location} [{finding.message}]"


if __name__ == "__main__":
    sys.exit(main())
