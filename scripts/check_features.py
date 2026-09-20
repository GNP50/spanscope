#!/usr/bin/env python3
"""Inspect isolated facade feature trees and build each declared feature."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def tree(features):
    command = ["cargo", "tree", "--locked", "-p", "spanscope", "--no-default-features",
               "-e", "normal,build", "--prefix", "none", "-f", "{p} features=[{f}]"]
    if features:
        command.extend(["--features", features])
    result = subprocess.check_output(command, cwd=ROOT, text=True)
    return result.splitlines()


disabled = tree("")
assert disabled[0].startswith("spanscope v") and "features=[]" in disabled[0], disabled
assert sum(line.startswith("spanscope-macros v") for line in disabled) == 1, disabled
assert not any(line.startswith(("serde v", "serde_json v", "schemars v"))
               for line in disabled), disabled
assert "features=[enabled]" not in next(line for line in disabled if line.startswith("spanscope-macros v"))
print("disabled graph: facade + disabled proc macro and its host-only parsing dependencies")

for feature in ("serialization", "schema", "enabled", "memory", "alloc-tracker", "ml", "viewer"):
    lines = tree(feature)
    facade = lines[0].split("features=[", 1)[1].rstrip("]").split(",")
    macro = next(line for line in lines if line.startswith("spanscope-macros v"))
    collecting = feature not in ("serialization", "schema")
    assert ("enabled" in facade) == collecting, (feature, lines)
    assert ("features=[enabled]" in macro) == collecting, (feature, macro)
    if feature == "alloc-tracker":
        assert "memory" in facade
    subprocess.run(["cargo", "check", "--locked", "-p", "spanscope", "--no-default-features",
                    "--features", feature], cwd=ROOT, check=True)
    print(f"checked feature contract: {feature}")

subprocess.run(["cargo", "test", "--locked", "-p", "spanscope", "--no-default-features"], cwd=ROOT, check=True)
subprocess.run(["cargo", "test", "--locked", "-p", "spanscope", "--no-default-features",
                "--features", "serialization"], cwd=ROOT, check=True)
