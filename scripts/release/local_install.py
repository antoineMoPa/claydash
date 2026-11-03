#!/usr/bin/env python3
"""Install a local release through the website installer, or remove that install."""
import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess

import release


def install():
    if platform.system() == "Windows":
        raise ValueError("Use install.ps1 on Windows; these Make targets support macOS and Linux")
    archive = release.build(f"v{release.version()}", release.host_target())
    env = dict(os.environ, CLAYDASH_DOWNLOAD_BASE_URL=archive.parent.resolve().as_uri())
    subprocess.run(["sh", str(release.ROOT / "install.sh")], env=env, check=True)


def uninstall():
    if platform.system() == "Windows":
        raise ValueError("These Make targets support macOS and Linux")
    install_dir = Path(os.environ.get("CLAYDASH_INSTALL_DIR") or Path.home() / ".local/bin")
    binary = install_dir / "claydash"
    binary.unlink(missing_ok=True)
    print(f"Removed {binary}")
    if platform.system() == "Darwin":
        app_dir = os.environ.get("CLAYDASH_APP_DIR")
        app_dirs = [Path(app_dir)] if app_dir else [Path("/Applications"), Path.home() / "Applications"]
        register = Path("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
        for directory in app_dirs:
            app = directory / "Claydash.app"
            if app.is_symlink():
                app.unlink()
            elif app.exists():
                if register.is_file():
                    subprocess.run([str(register), "-u", str(app)], check=False)
                shutil.rmtree(app)
            print(f"Removed {app}")
    print("Install cleared. Saved projects and application settings are preserved.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("install", "uninstall"))
    args = parser.parse_args()
    if args.action == "install":
        install()
    else:
        uninstall()


if __name__ == "__main__":
    main()
