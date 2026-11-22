#!/usr/bin/env python3
"""Benchmark PlicZip compression on real binary files larger than 100 MiB.

The input files are deliberately supplied by path and are never copied into the
repository. Each archive is created in a temporary directory, extracted, and
validated against the input SHA-256 digest.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

MIN_SIZE = 100 * 1024 * 1024
METHODS = (
    ("store", ["--level", "0"]),
    ("deflate-q4", ["--level", "4"]),
    ("brotli-q5", ["--level", "5"]),
    ("brotli-q11", ["--level", "11"]),
    ("density", ["--density", "--level", "5"]),
)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_command(command: list[str]) -> float:
    started = time.perf_counter()
    subprocess.run(command, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    return time.perf_counter() - started


def validate_inputs(paths: list[Path]) -> list[dict[str, object]]:
    inputs = []
    for path in paths:
        path = path.expanduser().resolve()
        if not path.is_file():
            raise SystemExit(f"input is not a regular file: {path}")
        size = path.stat().st_size
        if size <= MIN_SIZE:
            raise SystemExit(
                f"input must be larger than 100 MiB: {path} ({size / 1024 / 1024:.1f} MiB)"
            )
        inputs.append({"path": str(path), "name": path.name, "size_bytes": size, "sha256": sha256_file(path)})
    return inputs


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pliczip", type=Path, required=True, help="release pliczip binary")
    parser.add_argument("--input", type=Path, action="append", required=True, help="real binary >100 MiB; repeatable")
    parser.add_argument("--output", type=Path, required=True, help="directory for CSV/JSON/report")
    parser.add_argument("--repetitions", type=int, default=3, help="runs per input/method (default: 3)")
    args = parser.parse_args()

    if args.repetitions < 1:
        raise SystemExit("--repetitions must be at least 1")
    pliczip = args.pliczip.expanduser().resolve()
    if not pliczip.is_file():
        raise SystemExit(f"pliczip binary not found: {pliczip}")

    inputs = validate_inputs(args.input)
    args.output.mkdir(parents=True, exist_ok=True)
    rows: list[dict[str, object]] = []
    started_at = datetime.now(timezone.utc).isoformat()

    with tempfile.TemporaryDirectory(prefix="pliczip-large-benchmark-") as temp_name:
        temp = Path(temp_name)
        for input_info in inputs:
            source = Path(str(input_info["path"]))
            source_size = int(input_info["size_bytes"])
            for method, flags in METHODS:
                for repetition in range(1, args.repetitions + 1):
                    work = temp / f"{source.stem}-{method}-{repetition}"
                    work.mkdir()
                    archive = work / "archive.plz"
                    extracted = work / "extracted"
                    extracted.mkdir()
                    pack_seconds = run_command(
                        [str(pliczip), "--quiet", "add", *flags, str(archive), str(source)]
                    )
                    archive_size = archive.stat().st_size
                    extract_seconds = run_command(
                        [str(pliczip), "--quiet", "extract", str(archive), "--output", str(extracted)]
                    )
                    restored = extracted / source.name
                    if not restored.is_file():
                        raise RuntimeError(f"missing extracted file: {restored}")
                    restored_sha256 = sha256_file(restored)
                    if restored_sha256 != input_info["sha256"]:
                        raise RuntimeError(
                            f"SHA-256 mismatch for {source}: {restored_sha256} != {input_info['sha256']}"
                        )
                    rows.append(
                        {
                            "input": source.name,
                            "input_path": str(source),
                            "input_size_bytes": source_size,
                            "input_sha256": input_info["sha256"],
                            "method": method,
                            "repetition": repetition,
                            "archive_size_bytes": archive_size,
                            "archive_ratio": archive_size / source_size,
                            "pack_seconds": pack_seconds,
                            "extract_seconds": extract_seconds,
                            "pack_mib_s": source_size / 1024 / 1024 / pack_seconds,
                            "extract_mib_s": source_size / 1024 / 1024 / extract_seconds,
                            "round_trip_sha256_ok": True,
                        }
                    )
                    print(
                        f"{source.name}: {method} #{repetition} OK — "
                        f"{archive_size / 1024 / 1024:.1f} MiB, "
                        f"pack {rows[-1]['pack_mib_s']:.1f} MiB/s, "
                        f"extract {rows[-1]['extract_mib_s']:.1f} MiB/s",
                        flush=True,
                    )

    summaries = []
    for input_info in inputs:
        for method, _ in METHODS:
            selected = [r for r in rows if r["input"] == input_info["name"] and r["method"] == method]
            summaries.append(
                {
                    "input": input_info["name"],
                    "method": method,
                    "archive_size_bytes_median": statistics.median(r["archive_size_bytes"] for r in selected),
                    "archive_ratio_median": statistics.median(r["archive_ratio"] for r in selected),
                    "pack_seconds_median": statistics.median(r["pack_seconds"] for r in selected),
                    "extract_seconds_median": statistics.median(r["extract_seconds"] for r in selected),
                    "pack_mib_s_median": statistics.median(r["pack_mib_s"] for r in selected),
                    "extract_mib_s_median": statistics.median(r["extract_mib_s"] for r in selected),
                }
            )

    metadata = {
        "started_at_utc": started_at,
        "finished_at_utc": datetime.now(timezone.utc).isoformat(),
        "pliczip": str(pliczip),
        "pliczip_version": subprocess.check_output([str(pliczip), "--version"], text=True).strip(),
        "repetitions": args.repetitions,
        "minimum_input_size_bytes": MIN_SIZE,
        "inputs": inputs,
        "methods": [name for name, _ in METHODS],
        "all_round_trip_sha256_checks_passed": all(r["round_trip_sha256_ok"] for r in rows),
        "rows": rows,
        "summaries": summaries,
    }
    json_path = args.output / "large-binary-results.json"
    csv_path = args.output / "large-binary-results.csv"
    json_path.write_text(json.dumps(metadata, ensure_ascii=False, indent=2) + "\n")
    with csv_path.open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=rows[0].keys())
        writer.writeheader()
        writer.writerows(rows)
    print(f"Wrote {json_path}")
    print(f"Wrote {csv_path}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as error:
        print(f"command failed with exit code {error.returncode}", file=sys.stderr)
        if error.stderr:
            print(error.stderr.decode(errors="replace"), file=sys.stderr)
        raise
