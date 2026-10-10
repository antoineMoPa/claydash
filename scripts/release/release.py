#!/usr/bin/env python3
"""Build and ship only explicitly listed native release files. Python 3.11+."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import time
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[2]
REPO = "antoineMoPa/claydash"
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
)


def run(*args, capture=False, env=None):
    result = subprocess.run(args, cwd=ROOT, check=True, text=True,
                            stdout=subprocess.PIPE if capture else None, env=env)
    return result.stdout.strip() if capture else ""


def version():
    return tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


def check_tag(tag):
    if not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
        raise ValueError("release tag must be vMAJOR.MINOR.PATCH")
    if tag != f"v{version()}":
        raise ValueError("tag must match Cargo.toml's package version")


def host_target():
    targets = {
        ("Darwin", "arm64"): "aarch64-apple-darwin",
        ("Linux", "x86_64"): "x86_64-unknown-linux-gnu",
        ("Linux", "aarch64"): "aarch64-unknown-linux-gnu",
        ("Windows", "AMD64"): "x86_64-pc-windows-msvc",
    }
    try:
        return targets[(platform.system(), platform.machine())]
    except KeyError:
        raise ValueError(f"unsupported build host: {platform.system()} {platform.machine()}") from None


def build_env():
    # Deliberate allowlist: do not pass personal tokens, application settings, .env,
    # RUSTFLAGS, or arbitrary build-time variables into the public executable.
    keys = ("PATH", "HOME", "TMPDIR", "TMP", "TEMP", "USERPROFILE", "SystemRoot",
            "SYSTEMROOT", "COMSPEC", "PATHEXT", "APPDATA", "LOCALAPPDATA", "PROGRAMFILES",
            "PROGRAMFILES(X86)", "windir", "NUMBER_OF_PROCESSORS", "RUSTUP_HOME", "CARGO_HOME",
            # MSVC/Windows SDK compiler paths, initialized by msvc-dev-cmd.
            "LIB", "LIBPATH", "INCLUDE", "VCToolsInstallDir", "VCINSTALLDIR",
            "VSINSTALLDIR", "WindowsSdkDir", "WindowsSDKVersion", "UCRTVersion")
    env = {key: os.environ[key] for key in keys if key in os.environ}
    if platform.system() == "Windows":
        tools = env.get("VCToolsInstallDir")
        if not tools:
            raise ValueError("run Windows builds from Developer PowerShell for Visual Studio (x64)")
        compiler_bin = Path(tools) / "bin" / "Hostx64" / "x64"
        if not (compiler_bin / "link.exe").is_file():
            raise ValueError(f"missing Microsoft linker: {compiler_bin / 'link.exe'}")
        # Git Bash prepends its own link.exe. Put MSVC first even for local publishing.
        env["PATH"] = str(compiler_bin) + ";" + env["PATH"]
    return env


def archive_name(target):
    extension = "zip" if target.endswith("windows-msvc") else "tar.gz"
    return f"claydash-{target}.{extension}"


def package(tag, target):
    check_tag(tag)
    binary_name = "claydash.exe" if target.endswith("windows-msvc") else "claydash"
    binary = ROOT / "target" / target / "release" / binary_name
    if not binary.is_file():
        raise ValueError(f"missing executable: {binary}")
    output = ROOT / "target" / "release-artifacts" / tag
    output.mkdir(parents=True, exist_ok=True)
    archive = output / archive_name(target)
    # Explicit file list; no source directory, build cache, credentials or .env.
    files = ((binary, binary_name), (ROOT / "LICENSE.md", "LICENSE.md"))
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            for source, name in files:
                bundle.write(source, name)
    else:
        with tarfile.open(archive, "w:gz") as bundle:
            for source, name in files:
                info = bundle.gettarinfo(str(source), arcname=name)
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                info.mode = 0o755 if name == binary_name else 0o644
                with source.open("rb") as stream:
                    bundle.addfile(info, stream)
    with archive.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n")
    print(archive)
    return archive


def build(tag, target):
    check_tag(tag)
    if target != host_target():
        raise ValueError("build on the target's native runner")
    run("cargo", "build", "--release", "--locked", "--bin", "claydash",
        "--target", target, "--target-dir", str(ROOT / "target"), env=build_env())
    binary_name = "claydash.exe" if target.endswith("windows-msvc") else "claydash"
    run(str(ROOT / "target" / target / "release" / binary_name), "--help", env=build_env())
    return package(tag, target)


def verify(directory):
    for target in TARGETS:
        name = archive_name(target)
        archive = directory / name
        checksum = (directory / (name + ".sha256")).read_text().split()
        with archive.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        if checksum != [digest, name]:
            raise ValueError(f"checksum mismatch: {name}")
        expected = {"claydash.exe" if target.endswith("windows-msvc") else "claydash", "LICENSE.md"}
        if name.endswith(".zip"):
            with zipfile.ZipFile(archive) as bundle:
                members = bundle.namelist()
                regular = all(not member.is_dir() for member in bundle.infolist())
        else:
            with tarfile.open(archive) as bundle:
                members = bundle.getnames()
                regular = all(member.isfile() for member in bundle.getmembers())
        if len(members) != 2 or set(members) != expected or not regular:
            raise ValueError(f"unexpected archive contents: {name}")
    print("All four platform archives and checksums verified.")


def gh(*args, capture=False):
    return run("gh", *args, "--repo", REPO, capture=capture)


def publish(new_version, resume):
    if not re.fullmatch(r"\d+\.\d+\.\d+", new_version):
        raise ValueError("version must be MAJOR.MINOR.PATCH")
    tag = f"v{new_version}"
    target = host_target()
    if run("git", "status", "--porcelain", capture=True):
        raise ValueError("commit your work first; publishing requires a clean worktree")
    run("gh", "auth", "status")
    if resume:
        check_tag(tag)
        if run("git", "rev-parse", f"{tag}^{{commit}}", capture=True) != run("git", "rev-parse", "HEAD", capture=True):
            raise ValueError("resume from the release tag's exact commit")
        release = json.loads(gh("release", "view", tag, "--json", "isDraft", capture=True))
        if not release["isDraft"]:
            raise ValueError("resume requires an existing draft release")
    else:
        current = tuple(map(int, version().split(".")))
        if tuple(map(int, new_version.split("."))) <= current:
            raise ValueError("choose a version greater than Cargo.toml's current version")
        run("git", "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}", capture=True)
        tags = run("git", "ls-remote", "--tags", "origin", f"refs/tags/{tag}", capture=True)
        if tags or run("git", "tag", "--list", tag, capture=True):
            raise ValueError(f"tag {tag} already exists")
        run("git", "push", "--dry-run", "origin", "HEAD")
        manifest = ROOT / "Cargo.toml"
        text = manifest.read_text()
        start = text.index("[package]\n")
        end = text.index("\n[", start + 1)
        block, count = re.subn(r'^version\s*=\s*"[^\"]+"', f'version = "{new_version}"', text[start:end], count=1, flags=re.M)
        if count != 1:
            raise ValueError("could not update package version")
        manifest.write_text(text[:start] + block + text[end:])
        run("cargo", "update", "--workspace", "--offline", env=build_env())
        # Finish the local build before creating any commit/tag/release.
        build(tag, target)
        run("git", "add", "Cargo.toml", "Cargo.lock")
        run("git", "commit", "-m", f"chore(release): {tag}")
        run("git", "tag", "-a", tag, "-m", tag)
        run("git", "push", "--atomic", "origin", "HEAD", f"refs/tags/{tag}")
        gh("release", "create", tag, "--verify-tag", "--draft", "--title", tag,
           "--notes", f"Claydash {new_version}: native macOS Apple Silicon, Linux x64/ARM64, and Windows x64 builds.")
    if resume:
        build(tag, target)
    archive = ROOT / "target" / "release-artifacts" / tag / archive_name(target)
    gh("release", "upload", tag, str(archive), str(archive) + ".sha256", "--clobber")
    # Installer scripts are versioned release assets, too.
    gh("release", "upload", tag, str(ROOT / "install.sh"), str(ROOT / "install.ps1"), "--clobber")
    previous_runs = json.loads(gh("run", "list", "--workflow", "native-release.yml", "--branch", tag,
                                  "--event", "workflow_dispatch", "--json", "databaseId", "--limit", "20", capture=True))
    previous_ids = {job["databaseId"] for job in previous_runs}
    gh("workflow", "run", "native-release.yml", "--ref", tag, "-f", f"local_target={target}")
    run_id = None
    for _ in range(60):
        runs = json.loads(gh("run", "list", "--workflow", "native-release.yml", "--branch", tag,
                             "--event", "workflow_dispatch", "--json", "databaseId", "--limit", "20", capture=True))
        for job in runs:
            if job["databaseId"] not in previous_ids:
                run_id = str(job["databaseId"])
                break
        if run_id:
            break
        time.sleep(2)
    if not run_id:
        raise ValueError("workflow did not appear; draft remains unpublished")
    gh("run", "watch", run_id, "--exit-status")
    # Download into an empty directory so stale local files cannot mask missing uploads.
    output = ROOT / "target" / "release-artifacts" / tag / "downloaded"
    if output.exists():
        shutil.rmtree(output)
    gh("release", "download", tag, "--dir", str(output))
    verify(output)
    for name in ("install.sh", "install.ps1"):
        if (output / name).read_bytes() != (ROOT / name).read_bytes():
            raise ValueError(f"uploaded installer differs: {name}")
    gh("release", "edit", tag, "--draft=false", "--latest")
    print(f"Published https://github.com/{REPO}/releases/tag/{tag}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("build", "package"):
        p = sub.add_parser(command)
        p.add_argument("tag")
        p.add_argument("target", choices=TARGETS)
    p = sub.add_parser("verify")
    p.add_argument("directory", type=Path)
    p = sub.add_parser("publish")
    p.add_argument("version")
    p.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    if args.command == "build":
        build(args.tag, args.target)
    elif args.command == "package":
        package(args.tag, args.target)
    elif args.command == "verify":
        verify(args.directory)
    else:
        publish(args.version, args.resume)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(f"Release stopped: {error}\nAny draft remains unpublished. See docs/releases.md for recovery.")
