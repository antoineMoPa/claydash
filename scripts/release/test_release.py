#!/usr/bin/env python3
"""Offline release and installer checks; all artifacts stay in a temp directory."""
import contextlib
import functools
import io
import hashlib
import http.server
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import tarfile
import unittest
from unittest.mock import patch

import release


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_):
        pass


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.original_root = release.ROOT
        self.root_patch = patch.object(release, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        (self.root / "Cargo.toml").write_text('[package]\nname = "claydash"\nversion = "0.1.0"\n')
        (self.root / "LICENSE.md").write_text("test license\n")
        # Deliberately put private data beside the files we package.
        (self.root / ".env").write_text("PRIVATE_TOKEN=must-never-ship\n")
        for target in release.TARGETS:
            name = "claydash.exe" if target.endswith("windows-msvc") else "claydash"
            binary = self.root / "target" / target / "release" / name
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"test executable\n")
            with contextlib.redirect_stdout(io.StringIO()):
                release.package("v0.1.0", target)
        self.output = self.root / "target/release-artifacts/v0.1.0"

    def test_archive_allowlist_and_corruption(self):
        release.verify(self.output)
        archive = self.output / release.archive_name(release.TARGETS[0])
        archive.write_bytes(archive.read_bytes() + b"corruption")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            release.verify(self.output)

    def test_unexpected_files_prevent_publication(self):
        archive = self.output / release.archive_name(release.TARGETS[0])
        with tarfile.open(archive, "w:gz") as bundle:
            bundle.add(self.root / ".env", arcname=".env")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n")
        with self.assertRaisesRegex(ValueError, "unexpected archive contents"):
            release.verify(self.output)

    def test_resume_never_publishes_incomplete_release(self):
        calls = []
        for name in ("install.sh", "install.ps1"):
            shutil.copyfile(self.original_root / name, self.root / name)
        def fake_run(*args, **kwargs):
            if args[:2] == ("git", "rev-parse"):
                return "same-commit"
            return ""
        def fake_gh(*args, **kwargs):
            calls.append(args)
            if args[:2] == ("release", "view"):
                return '{"isDraft": true}'
            if args[:2] == ("run", "list"):
                return "[]" if sum(c[:2] == ("run", "list") for c in calls) == 1 else '[{"databaseId": 123}]'
            if args[:2] == ("release", "download"):
                destination = Path(args[args.index("--dir") + 1])
                destination.mkdir()
                for asset in self.output.iterdir():
                    if asset.is_file() and "windows" not in asset.name:
                        shutil.copyfile(asset, destination / asset.name)
            return ""
        with patch.object(release, "run", side_effect=fake_run), \
             patch.object(release, "gh", side_effect=fake_gh), \
             patch.object(release, "host_target", return_value=release.TARGETS[0]), \
             patch.object(release, "build"):
            with self.assertRaises(FileNotFoundError):
                release.publish("0.1.0", resume=True)
        self.assertTrue(any(c[:2] == ("workflow", "run") and f"local_target={release.TARGETS[0]}" in c for c in calls))
        self.assertFalse(any(c[:2] == ("release", "edit") for c in calls))

    def test_missing_platform_prevents_publication(self):
        (self.output / release.archive_name(release.TARGETS[-1])).unlink()
        with self.assertRaises(FileNotFoundError):
            release.verify(self.output)

    def test_build_environment_excludes_secrets(self):
        with patch.dict(os.environ, {"PRIVATE_TOKEN": "private", "RUSTFLAGS": "private", "CLAYDASH_AGENT_SOCKET": "private"}):
            env = release.build_env()
        self.assertNotIn("PRIVATE_TOKEN", env)
        self.assertNotIn("RUSTFLAGS", env)
        self.assertNotIn("CLAYDASH_AGENT_SOCKET", env)
        self.assertIn("PATH", env)

    def test_windows_microsoft_linker_precedes_git_tools(self):
        tools = self.root / "Visual Studio tools"
        compiler = tools / "bin/Hostx64/x64"
        compiler.mkdir(parents=True)
        (compiler / "link.exe").write_bytes(b"test linker")
        with patch.object(release.platform, "system", return_value="Windows"), \
             patch.dict(os.environ, {"VCToolsInstallDir": str(tools), "PATH": "git-tools;rust-tools", "LIB": "sdk-libraries"}):
            env = release.build_env()
        self.assertEqual(env["PATH"], str(compiler) + ";git-tools;rust-tools")
        self.assertEqual(env["LIB"], "sdk-libraries")

    def test_tag_must_match_package(self):
        with self.assertRaises(ValueError):
            release.check_tag("v0.2.0")
        with self.assertRaises(ValueError):
            release.check_tag("../../private")

    def serve(self):
        handler = functools.partial(QuietHandler, directory=str(self.output))
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return f"http://127.0.0.1:{server.server_port}"

    @unittest.skipIf(os.name == "nt", "Unix installer")
    def test_unix_installer_success_upgrade_corruption_and_unsupported(self):
        base = self.serve()
        shims = self.root / "shims"
        shims.mkdir()
        uname = shims / "uname"
        uname.write_text('#!/bin/sh\ncase "$1" in -s) echo "$TEST_OS";; -m) echo "$TEST_ARCH";; esac\n')
        uname.chmod(0o755)
        script = (self.original_root / "install.sh").read_text()
        cases = (("Darwin", "arm64", release.TARGETS[0]),
                 ("Linux", "x86_64", release.TARGETS[1]),
                 ("Linux", "aarch64", release.TARGETS[2]))
        for system, arch, target in cases:
            with self.subTest(target=target):
                dest = self.root / f"install space {arch} {system}"
                env = dict(os.environ, CLAYDASH_INSTALL_DIR=str(dest), CLAYDASH_DOWNLOAD_BASE_URL=base,
                           PATH=f"{shims}{os.pathsep}{os.environ['PATH']}", TEST_OS=system, TEST_ARCH=arch)
                def install():
                    # stdin exercises the actual curl | sh execution shape.
                    return subprocess.run(["sh"], input=script, env=env, text=True, capture_output=True)
                first = install()
                self.assertEqual(first.returncode, 0, first.stderr)
                self.assertEqual((dest / "claydash").read_bytes(), b"test executable\n")
                self.assertTrue(os.access(dest / "claydash", os.X_OK))
                (dest / "claydash").write_bytes(b"old version")
                self.assertEqual(install().returncode, 0)
                archive = self.output / release.archive_name(target)
                original = archive.read_bytes()
                archive.write_bytes(b"corrupt")
                failed = install()
                self.assertNotEqual(failed.returncode, 0)
                self.assertIn("Checksum verification failed", failed.stderr)
                self.assertEqual((dest / "claydash").read_bytes(), b"test executable\n")
                archive.write_bytes(original)
        env["TEST_OS"], env["TEST_ARCH"] = "Darwin", "x86_64"
        failed = subprocess.run(["sh"], input=script, env=env, text=True, capture_output=True)
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("Supported:", failed.stderr)

    @unittest.skipUnless(os.name == "nt", "Windows installer runs on Windows CI")
    def test_windows_installer_success_and_corruption(self):
        base = self.serve()
        dest = self.root / "install space"
        env = dict(os.environ, CLAYDASH_INSTALL_DIR=str(dest), CLAYDASH_DOWNLOAD_BASE_URL=base,
                   TEST_INSTALLER=str(self.original_root / "install.ps1"))
        # Restore user PATH after checking the installer on the ephemeral runner.
        command = """
$ErrorActionPreference = 'Stop'
$previous = [Environment]::GetEnvironmentVariable('Path', 'User')
try { & $env:TEST_INSTALLER; if (-not (Test-Path "$env:CLAYDASH_INSTALL_DIR\\claydash.exe")) { throw 'Missing binary' } }
catch { Write-Error $_; exit 1 }
finally { [Environment]::SetEnvironmentVariable('Path', $previous, 'User') }
"""
        shell = shutil.which("pwsh") or "powershell"
        def install():
            return subprocess.run([shell, "-NoProfile", "-Command", command], env=env, text=True, capture_output=True)
        first = install()
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual((dest / "claydash.exe").read_bytes(), b"test executable\n")
        archive = self.output / release.archive_name(release.TARGETS[-1])
        archive.write_bytes(b"corrupt")
        failed = install()
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("Checksum verification failed", failed.stderr)
        self.assertEqual((dest / "claydash.exe").read_bytes(), b"test executable\n")


if __name__ == "__main__":
    unittest.main()
