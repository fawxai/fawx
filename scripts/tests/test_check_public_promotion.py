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
                "app/Fawx/SecretView.swift": "let token = \"hidden\"\n",
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
                    "pub const REPO: &str = \"https://github.com/abbudjoe/fawx\";\n"
                )
            },
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Private markers:", result.stdout)
        self.assertIn("private repo reference", result.stdout)

    def test_public_author_invariant_fails(self) -> None:
        repo = self.prepare_repo(
            base_files={"Cargo.toml": "[workspace.package]\nauthors = [\"Fawx AI\"]\n"},
            changed_files={"Cargo.toml": "[workspace.package]\nauthors = [\"Joe\"]\n"},
        )

        result = self.run_guard(repo)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Public invariants:", result.stdout)
        self.assertIn("author metadata should stay public-safe", result.stdout)

    def test_safe_promotion_passes(self) -> None:
        repo = self.prepare_repo(
            base_files={
                "Cargo.toml": "[workspace.package]\nauthors = [\"Fawx AI\"]\n",
                "engine/crates/fx-core/src/lib.rs": "pub fn base() {}\n",
            },
            changed_files={
                "engine/crates/fx-core/src/lib.rs": (
                    "pub fn base() {}\n"
                    "pub fn public_change() -> &'static str {\n"
                    "    \"ready\"\n"
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
