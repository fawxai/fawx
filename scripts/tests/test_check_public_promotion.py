#!/usr/bin/env python3
from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SOURCE_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(SOURCE_ROOT / "scripts"))

import check_public_promotion as public_promotion_guard

GUARD_FILES = [
    "scripts/check-public-promotion",
    "scripts/check-public-promotion.toml",
    "scripts/check_public_promotion.py",
]
APP_POLICY_PATHS = (
    "app/Fawx/Models/ServerStatus.swift",
    "app/Fawx/Networking/FawxClient.swift",
    "app/Fawx/Utilities/LocalInstallConfiguration.swift",
    "app/Fawx/ViewModels/SetupViewModel.swift",
    "app/Fawx/Views/Shared/PairingSettingsPanel.swift",
    "app/Fawx/Views/Shared/SetupWizard/TailscaleStep.swift",
    "app/Fawx/FawxApp.swift",
    "app/Fawx/Services/LocalBootstrapService.swift",
    "app/Fawx/Views/Shared/OnboardingView.swift",
    "app/Fawx/Views/iOS/iOSSettingsView.swift",
    "app/FawxTests/ViewModels/SettingsViewModelTests.swift",
    "app/FawxTests/Utilities/FormattersTests.swift",
    "app/FawxUITests/PairingFlowTests.swift",
)
WORKFLOW_PREFIX = (
    "name: CI\n"
    "on: [push]\n"
    "jobs:\n"
    "  check:\n"
    "    runs-on: ubuntu-latest\n"
    "    steps:\n"
    "      - run: "
)


def workflow_file(command: str) -> str:
    return WORKFLOW_PREFIX + command + "\n"


PRIVATE_REPO_URL = "https://github.com/abbudjoe" "/fawx"
PRIVATE_REPO_SLUG = "abbudjoe" "/fawx"
FAKE_GITHUB_TOKEN = "ghp_" "TESTTOKEN1234567890ABCD"
TAILSCALE_HOST = "relay.tail9696fb" ".ts.net"
TAILSCALE_SWIFT_HOST = "alice-macbook.tail9696fb" ".ts.net"
TAILSCALE_IP = "100." "93.251.101"
RFC1918_URL = "http://192.168." "1.10:8400"
PRIVATE_ASSISTANT = "claw" "dio"
PRIVATE_LOCAL_PATH = "/Users/" "joseph/fawx"


def load_guard_config() -> public_promotion_guard.GuardConfig:
    return public_promotion_guard.load_config(
        SOURCE_ROOT / "scripts/check-public-promotion.toml"
    )


def app_allowlist_patterns(
    config: public_promotion_guard.GuardConfig,
) -> tuple[str, ...]:
    return tuple(pattern for pattern in config.allowlist if pattern.startswith("app/"))


def iter_policy_files(repo_root: Path, patterns: tuple[str, ...]) -> tuple[Path, ...]:
    matches: set[Path] = set()
    for pattern in patterns:
        if any(character in pattern for character in "*?[]"):
            matches.update(path for path in repo_root.glob(pattern) if path.is_file())
            continue
        candidate = repo_root / pattern
        if candidate.is_file():
            matches.add(candidate)
    return tuple(sorted(matches, key=lambda path: path.as_posix()))


class CheckPublicPromotionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.repo_root = Path(self.temp_dir.name) / "repo"
        self.repo_root.mkdir()

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_blocked_private_path_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n"},
            changed_files={
                "docs/strategy/private.md": 'let token = "hidden"\n',
                "engine/crates/fx-core/src/lib.rs": "pub fn base() {}\npub fn next() {}\n",
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Blocked paths:", result.stdout)
        self.assertIn("docs/strategy/private.md", result.stdout)

    def test_reviewed_app_path_passes(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={
                "app/Fawx/ViewModels/SetupViewModel.swift": (
                    "struct SetupViewModel {\n"
                    '    let status = "ready"\n'
                    "}\n"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("Blocked paths:", result.stdout)
        self.assertNotIn("Allowlist misses:", result.stdout)

    def test_allowlisted_shared_view_path_passes_despite_broader_shared_blocklist(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={
                "app/Fawx/Views/Shared/PairingSettingsPanel.swift": (
                    "struct PairingSettingsPanel {\n"
                    '    let status = "ready"\n'
                    "}\n"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("Blocked paths:", result.stdout)
        self.assertNotIn("Allowlist misses:", result.stdout)

    def test_allowlist_miss_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n"},
            changed_files={"notes/private.md": "should not go public\n"},
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Allowlist misses:", result.stdout)
        self.assertIn("notes/private.md", result.stdout)

    def test_private_marker_in_added_line_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n"},
            changed_files={
                "engine/crates/fx-core/src/lib.rs": (
                    "pub fn base() {}\n"
                    f'pub const REPO: &str = "{PRIVATE_REPO_URL}";\n'
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("private repo reference", result.stdout)

    def test_credential_token_marker_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n"},
            changed_files={
                "engine/crates/fx-core/src/lib.rs": (
                    "pub fn base() {}\n"
                    f'pub const TOKEN: &str = "{FAKE_GITHUB_TOKEN}";\n'
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("suspicious credential token", result.stdout)

    def test_tailscale_hostname_marker_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n"},
            changed_files={
                "engine/crates/fx-core/src/lib.rs": (
                    "pub fn base() {}\n"
                    f'pub const HOST: &str = "{TAILSCALE_HOST}";\n'
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("Tailscale hostname", result.stdout)

    def test_tailscale_ipv4_marker_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n"},
            changed_files={
                "engine/crates/fx-core/src/lib.rs": (
                    "pub fn base() {}\n"
                    f'pub const HOST: &str = "{TAILSCALE_IP}";\n'
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("Tailscale IPv4", result.stdout)

    def test_private_rfc1918_ipv4_marker_in_source_file_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n"},
            changed_files={
                "engine/crates/fx-core/src/lib.rs": (
                    "pub fn base() {}\n"
                    f'pub const HOST: &str = "{RFC1918_URL}";\n'
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("RFC1918 private IPv4", result.stdout)

    def test_private_rfc1918_ipv4_marker_in_allowed_app_file_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={
                "app/Fawx/ViewModels/SetupViewModel.swift": (
                    "struct SetupViewModel {\n"
                    f'    let serverURL = "{RFC1918_URL}"\n'
                    "}\n"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("RFC1918 private IPv4", result.stdout)

    def test_tailscale_ipv4_marker_in_test_fixture_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/tests/network_fixture.rs": "fn baseline() {}\n"},
            changed_files={
                "engine/crates/fx-core/tests/network_fixture.rs": (
                    "fn baseline() {}\n"
                    f'const TAILNET_IP: &str = "{TAILSCALE_IP}";\n'
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("Tailscale IPv4", result.stdout)

    def test_tailscale_ipv4_marker_in_allowed_app_test_file_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={
                "app/FawxTests/ViewModels/SetupViewModelTests.swift": (
                    "func testSetupHost() {\n"
                    f'    let host = "{TAILSCALE_IP}"\n'
                    "}\n"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("Tailscale IPv4", result.stdout)

    def test_private_hostname_marker_in_swift_fixture_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"scripts/tests/fixtures/ServerStatus.swift": "let title = \"Ready\"\n"},
            changed_files={
                "scripts/tests/fixtures/ServerStatus.swift": (
                    "let title = \"Ready\"\n"
                    f'let endpoint = "https://{TAILSCALE_SWIFT_HOST}:8400"\n'
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("Tailscale hostname", result.stdout)

    def test_private_hostname_marker_in_allowed_app_file_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={
                "app/Fawx/Views/Shared/PairingSettingsPanel.swift": (
                    "struct PairingSettingsPanel {\n"
                    f'    let host = "{TAILSCALE_HOST}"\n'
                    "}\n"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("Tailscale hostname", result.stdout)

    def test_private_local_user_path_marker_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={
                "app/FawxTests/Models/SessionTests.swift": (
                    "func testWorkspacePath() {\n"
                    f'    let path = "{PRIVATE_LOCAL_PATH}"\n'
                    "}\n"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("private local user path", result.stdout)

    def test_loopback_and_generic_tailscale_references_pass(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n"},
            changed_files={
                "engine/crates/fx-core/src/lib.rs": (
                    "pub fn base() {}\n"
                    "pub fn setup_copy() -> &'static str {\n"
                    '    "Connect with Tailscale or use http://127.0.0.1:8400 (localhost only)."\n'
                    "}\n"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("Private markers:", result.stdout)

    def test_loopback_and_generic_tailscale_references_in_allowed_app_file_pass(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={
                "app/Fawx/Views/Shared/SetupWizard/TailscaleStep.swift": (
                    "struct TailscaleStep {\n"
                    '    let copy = "Connect with Tailscale or use http://127.0.0.1:8400."\n'
                    "}\n"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("Private markers:", result.stdout)

    def test_invalid_rfc1918_like_ipv4_does_not_match(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n"},
            changed_files={
                "engine/crates/fx-core/src/lib.rs": (
                    "pub fn base() {}\n"
                    'pub const HOST: &str = "http://192.168.300.1:8400";\n'
                )
            },
        )

        result = self.run_guard(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("RFC1918 private IPv4", result.stdout)

    def test_public_author_invariant_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"Cargo.toml": '[workspace.package]\nauthors = ["Fawx AI"]\n'},
            changed_files={"Cargo.toml": '[workspace.package]\nauthors = ["Joe"]\n'},
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Public invariants:", result.stdout)
        self.assertIn("author metadata should stay public-safe", result.stdout)

    def test_workflow_private_ip_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={".github/workflows/ci.yml": workflow_file(f"echo {RFC1918_URL}")},
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Public invariants:", result.stdout)
        self.assertIn("public workflow references internal IP address", result.stdout)

    def test_workflow_tailscale_endpoint_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={
                ".github/workflows/ci.yml": workflow_file(
                    f"echo wss://{PRIVATE_ASSISTANT}.{TAILSCALE_HOST}/socket"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Public invariants:", result.stdout)
        self.assertIn(
            "public workflow references private host or tailnet endpoint",
            result.stdout,
        )

    def test_public_workflow_websocket_url_passes(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={".github/workflows/ci.yml": workflow_file("echo wss://example.com/socket")},
        )

        result = self.run_guard(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("Public invariants:", result.stdout)

    def test_workflow_localhost_ip_passes(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={".github/workflows/ci.yml": workflow_file("echo 127.0.0.1")},
        )

        result = self.run_guard(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("Public invariants:", result.stdout)

    def test_broad_promotion_warns(self) -> None:
        changed_files = {
            f"engine/crates/fx-core/src/file_{index}.rs": f"pub fn item_{index}() {{}}\n"
            for index in range(41)
        }
        repo = self.prepare_repo(base_files={}, changed_files=changed_files)

        result = self.run_guard(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Warnings:", result.stdout)
        self.assertIn("41 changed files across 1 top-level areas", result.stdout)

    def test_wide_promotion_warns(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={
                ".cargo/config.toml": "[build]\n",
                "assets/logo.txt": "logo\n",
                "bindings/public.h": "// header\n",
                "engine/crates/fx-core/src/lib.rs": "pub fn public_change() {}\n",
                "tui/src/main.rs": "fn main() {}\n",
            },
        )

        result = self.run_guard(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Warnings:", result.stdout)
        self.assertIn("5 changed files across 5 top-level areas", result.stdout)

    def test_missing_base_ref_fails_loudly(self) -> None:
        repo = self.prepare_repo(
            base_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n"},
            changed_files={"engine/crates/fx-core/src/lib.rs": "pub fn base() {}\npub fn next() {}\n"},
        )
        config_path = repo / "scripts/check-public-promotion.toml"
        config_text = config_path.read_text(encoding="utf-8")
        config_path.write_text(
            config_text.replace(
                'base_ref = "public/main"',
                'base_ref = "public/missing"',
                1,
            ),
            encoding="utf-8",
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("check-public-promotion: FAIL", result.stdout)
        self.assertIn("Base ref 'public/missing' is missing", result.stdout)

    def test_safe_promotion_passes(self) -> None:
        repo = self.prepare_repo(
            base_files={
                "Cargo.toml": '[workspace.package]\nauthors = ["Fawx AI"]\n',
                "engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n",
            },
            changed_files={
                "engine/crates/fx-core/src/lib.rs": (
                    "pub fn base() {}\n"
                    "pub fn public_change() -> &'static str {\n"
                    '    "ready"\n'
                    "}\n"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("check-public-promotion: PASS", result.stdout)

    def test_app_paths_are_allowlisted_and_unblocked(self) -> None:
        config = load_guard_config()
        blocked_paths = public_promotion_guard.find_blocked_paths(
            APP_POLICY_PATHS,
            config.allowlist,
            config.blocklist,
        )
        allowlist_misses = public_promotion_guard.find_allowlist_misses(
            APP_POLICY_PATHS,
            config.allowlist,
            config.blocklist,
        )

        self.assertEqual(blocked_paths, ())
        self.assertEqual(allowlist_misses, ())

    def test_app_blocklist_is_empty_after_full_app_promotion(self) -> None:
        config = load_guard_config()
        self.assertEqual(
            tuple(pattern for pattern in config.blocklist if pattern.startswith("app/")),
            (),
        )

    def test_public_app_tree_contains_no_private_network_literals(self) -> None:
        config = load_guard_config()
        forbidden_patterns = tuple(
            pattern
            for pattern in config.marker_patterns
            if pattern.name in {
                "Tailscale hostname",
                "Tailscale IPv4",
                "RFC1918 private IPv4",
            }
        )

        findings: list[str] = []
        app_files = iter_policy_files(
            SOURCE_ROOT,
            app_allowlist_patterns(config),
        )
        self.assertGreater(len(app_files), 0, "expected public app targets")

        for file_path in app_files:
            relative_path = file_path.relative_to(SOURCE_ROOT).as_posix()
            try:
                content = file_path.read_text(encoding="utf-8")
            except UnicodeDecodeError:
                continue
            for line_number, line in enumerate(content.splitlines(), start=1):
                match = public_promotion_guard.first_named_match(line, forbidden_patterns)
                if match is None:
                    continue
                findings.append(f"{relative_path}:{line_number} [{match.name}] {line.strip()}")

        self.assertEqual(findings, [], "\n".join(findings))

    def prepare_repo(
        self,
        base_files: dict[str, str],
        changed_files: dict[str, str],
    ) -> Path:
        self.copy_guard_files(self.repo_root)
        self.git("init")
        self.git("config", "user.email", "tests@example.com")
        self.git("config", "user.name", "Promotion Guard Tests")
        self.write_files(base_files)
        self.git("add", ".")
        self.git("commit", "-m", "base")
        self.git("branch", "public/main")
        self.git("checkout", "-b", "promote/test", "public/main")
        self.write_files(changed_files)
        self.git("add", "-A")
        self.git("commit", "-m", "candidate")
        return self.repo_root

    def copy_guard_files(self, repo_root: Path) -> None:
        for relative_path in GUARD_FILES:
            source = SOURCE_ROOT / relative_path
            target = repo_root / relative_path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)

    def write_files(self, files: dict[str, str]) -> None:
        for relative_path, content in files.items():
            target = self.repo_root / relative_path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding="utf-8")

    def git(self, *args: str) -> None:
        subprocess.run(
            ["git", *args],
            cwd=self.repo_root,
            text=True,
            capture_output=True,
            check=True,
        )

    def run_guard(self, repo_root: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(repo_root / "scripts/check-public-promotion")],
            cwd=repo_root,
            text=True,
            capture_output=True,
            check=False,
        )


if __name__ == "__main__":
    unittest.main()
