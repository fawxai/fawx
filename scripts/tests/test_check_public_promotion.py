#!/usr/bin/env python3
from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SOURCE_ROOT = Path(__file__).resolve().parents[2]
GUARD_FILES = [
    "scripts/check-public-promotion",
    "scripts/check-public-promotion.toml",
    "scripts/check_public_promotion.py",
]
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
                "app/Fawx/SecretView.swift": 'let token = "hidden"\n',
                "engine/crates/fx-core/src/lib.rs": "pub fn base() {}\npub fn next() {}\n",
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Blocked paths:", result.stdout)
        self.assertIn("app/Fawx/SecretView.swift", result.stdout)

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
                    'pub const REPO: &str = "https://github.com/abbudjoe/fawx";\n'
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
                    'pub const TOKEN: &str = "ghp_TESTTOKEN1234567890ABCD";\n'
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
                    'pub const HOST: &str = "relay.tail9696fb.ts.net";\n'
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
                    'pub const HOST: &str = "100.89.174.76";\n'
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("Tailscale IPv4", result.stdout)

    def test_public_author_invariant_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"Cargo.toml": '[workspace.package]\nauthors = ["Fawx AI"]\n'},
            changed_files={"Cargo.toml": '[workspace.package]\nauthors = ["Joe"]\n'},
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Public invariants:", result.stdout)
        self.assertIn("author metadata should stay public-safe", result.stdout)

    def test_llama_reintroduction_fails_when_absent_from_base(self) -> None:
        repo = self.prepare_repo(
            base_files={
                "engine/crates/fx-core/Cargo.toml": (
                    "[package]\n"
                    'name = "fx-core"\n'
                    'version = "0.1.0"\n'
                )
            },
            changed_files={
                "engine/crates/fx-core/Cargo.toml": (
                    "[package]\n"
                    'name = "fx-core"\n'
                    'version = "0.1.0"\n\n'
                    "[dependencies]\n"
                    'llama-cpp-sys = "0.1"\n'
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Public invariants:", result.stdout)
        self.assertIn("llama-cpp-sys is absent from the base ref", result.stdout)

    def test_workflow_private_ip_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={},
            changed_files={".github/workflows/ci.yml": workflow_file("echo 10.1.2.3")},
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
                    "echo wss://clawdio.tail9696fb.ts.net/socket"
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
