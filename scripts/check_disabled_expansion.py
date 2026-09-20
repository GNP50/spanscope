#!/usr/bin/env python3
"""Compare cargo-expand output for the disabled annotated const function."""
import os
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
expand = os.environ.get("CARGO_EXPAND") or shutil.which("cargo-expand")
if not expand:
    raise SystemExit("cargo-expand is required for this snapshot check")
expanded = subprocess.check_output(
    [expand, "expand", "-p", "spanscope", "--test", "disabled",
     "--no-default-features", "--color", "never"],
    cwd=ROOT,
    text=True,
)
match = re.search(
    r"^const fn unchanged\(value: u32\) -> u32 \{\n    value \+ 1\n\}",
    expanded,
    flags=re.MULTILINE,
)
assert match, "disabled annotated function missing or altered in expansion"
actual = match.group(0) + "\n"
expected = (ROOT / "spanscope/tests/snapshots/disabled_expanded.txt").read_text()
assert actual == expected, "disabled expansion differs from committed snapshot"
assert "__private::enter" not in expanded, "disabled target contains runtime entry call"
print("disabled const function expansion matches original tokens")
