from __future__ import annotations

import argparse
import json
from pathlib import Path


ALLOWED_CONTROL_BYTES = {0x09, 0x0A, 0x0D}
KNOWN_BOMS = (
    b"\x00\x00\xfe\xff",
    b"\xff\xfe\x00\x00",
    b"\xef\xbb\xbf",
    b"\xff\xfe",
    b"\xfe\xff",
)


def discover_g04dc_powershell_sources(source_root: Path) -> list[Path]:
    return sorted(
        (
            path
            for path in source_root.rglob("*")
            if path.is_file() and path.suffix.lower() in {".ps1", ".psm1"}
        ),
        key=lambda path: (path.as_posix().casefold(), path.as_posix()),
    )


def find_forbidden_offsets(contents: bytes) -> list[int]:
    forbidden = [
        offset
        for offset, value in enumerate(contents)
        if value not in ALLOWED_CONTROL_BYTES and not 0x20 <= value <= 0x7E
    ]
    if any(contents.startswith(bom) for bom in KNOWN_BOMS) and 0 not in forbidden:
        forbidden.insert(0, 0)
    return forbidden


def validate_g04dc_powershell_source_bytes(
    repository_root: Path,
    source_root: Path,
) -> dict[str, object]:
    repository_root = repository_root.resolve()
    source_root = source_root.resolve()
    source_root.relative_to(repository_root)

    relative_files: list[str] = []
    violations: list[dict[str, object]] = []
    for path in discover_g04dc_powershell_sources(source_root):
        relative_path = path.resolve().relative_to(repository_root).as_posix()
        relative_files.append(relative_path)
        for offset in find_forbidden_offsets(path.read_bytes()):
            violations.append({"path": relative_path, "offset": offset})

    return {
        "schemaVersion": 1,
        "sourceFileCount": len(relative_files),
        "sourceFiles": relative_files,
        "asciiGateStatus": "PASS" if not violations else "FAIL",
        "violations": violations,
    }


def format_violations(report: dict[str, object]) -> str:
    return ", ".join(
        f"{entry['path']} byte offset {entry['offset']}"
        for entry in report["violations"]
    )


def main() -> int:
    default_repository_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository-root", type=Path, default=default_repository_root)
    parser.add_argument(
        "--source-root",
        type=Path,
        default=default_repository_root / "scripts" / "g04d-c",
    )
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    report = validate_g04dc_powershell_source_bytes(
        args.repository_root,
        args.source_root,
    )
    if args.json:
        print(json.dumps(report, ensure_ascii=True, separators=(",", ":")))
    elif report["violations"]:
        print(format_violations(report))
    else:
        print(
            "G04D-C ASCII source byte gate passed "
            f"({report['sourceFileCount']} files)."
        )
    return 0 if not report["violations"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
