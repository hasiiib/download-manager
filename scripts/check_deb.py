#!/usr/bin/env python3
"""Inspect a generated Debian package without installing or executing it."""
import pathlib
import re
import subprocess
import sys


def field(package: pathlib.Path, name: str) -> str:
    return subprocess.check_output(
        ["dpkg-deb", "--field", str(package), name],
        text=True,
    ).strip()


def package_paths(package: pathlib.Path) -> list[str]:
    listing = subprocess.check_output(
        ["dpkg-deb", "--fsys-tarfile", str(package)],
    )
    import io
    import tarfile
    with tarfile.open(fileobj=io.BytesIO(listing), mode="r:") as archive:
        return [member.name.removeprefix("./").lstrip("/") for member in archive]


def validate(package: pathlib.Path) -> None:
    if not package.is_file():
        raise ValueError(f"Debian package not found: {package}")
    name = field(package, "Package")
    version = field(package, "Version")
    dependencies = field(package, "Depends")
    print(f"Package: {name}\nVersion: {version}\nDepends: {dependencies}")
    if name != "download-manager":
        raise ValueError(f"Unexpected package name: {name}")
    if not re.fullmatch(r"0\\.2\\.0(?:[-+].+)?", version):
        raise ValueError(f"Unexpected package version: {version}")
    if not re.search(r"(?:^|,)\\s*aria2(?:\\s*\\([^)]*\\))?(?:\\s*,|$)", dependencies):
        raise ValueError("Installer does not declare aria2 dependency")

    paths = package_paths(package)
    required = {
        "main application executable": lambda p: p == "usr/bin/download-manager",
        "desktop launcher": lambda p: p.startswith("usr/share/applications/") and p.endswith(".desktop"),
        "application icon": lambda p: p.startswith("usr/share/icons/") and p.endswith(".png"),
    }
    for label, predicate in required.items():
        matched = next((path for path in paths if predicate(path)), None)
        if not matched:
            raise ValueError(f"Installer is missing {label}")
        print(f"[OK] {label}: {matched}")
    print("Debian installer structure verified.")


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: python3 scripts/check_deb.py PATH_TO.deb", file=sys.stderr)
        return 2
    try:
        validate(pathlib.Path(sys.argv[1]))
        return 0
    except (ValueError, subprocess.CalledProcessError, OSError) as error:
        print(f"[ERROR] {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
