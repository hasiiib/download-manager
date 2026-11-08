#!/usr/bin/env python3
"""Inspect a generated Debian package without installing or executing it."""
import json
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
    # Stream the uncompressed tar from dpkg-deb instead of buffering the
    # entire installer in RAM. Large app bundles can be hundreds of MB.
    import tarfile
    with subprocess.Popen(
        ["dpkg-deb", "--fsys-tarfile", str(package)],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    ) as process:
        assert process.stdout is not None
        assert process.stderr is not None
        try:
            with tarfile.open(fileobj=process.stdout, mode="r|") as archive:
                paths = [member.name.removeprefix("./").lstrip("/") for member in archive]
        except (tarfile.TarError, OSError) as error:
            process.kill()
            process.communicate()
            raise ValueError(f"Cannot read Debian archive {package}: {error}") from error
        error_output = process.stderr.read()
        if process.wait() != 0:
            raise ValueError(
                "Unable to inspect Debian archive: "
                + error_output.decode("utf-8", errors="replace").strip()
            )
    return paths


def validate(package: pathlib.Path) -> None:
    if not package.is_file():
        raise ValueError(f"Debian package not found: {package}")
    name = field(package, "Package")
    version = field(package, "Version")
    dependencies = field(package, "Depends")
    print(f"Package: {name}\nVersion: {version}\nDepends: {dependencies}")
    manifest_path = pathlib.Path(__file__).resolve().parents[1] / "package.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    expected_name = manifest["name"]
    expected_version = manifest["version"]
    if name != expected_name:
        raise ValueError(f"Unexpected package name: {name}; expected {expected_name}")
    if not re.fullmatch(re.escape(expected_version) + r"(?:[-+].+)?", version):
        raise ValueError(f"Unexpected package version: {version}; expected {expected_version}")
    if not re.search(r"(?:^|,)\s*aria2(?:\s*\([^)]*\))?(?:\s*,|$)", dependencies):
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
