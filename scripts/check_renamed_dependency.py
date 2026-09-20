#!/usr/bin/env python3
"""Compile an independent consumer that renames the spanscope dependency."""
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "target" / "renamed-consumer"
SOURCE = PROJECT / "src"
SOURCE.mkdir(parents=True, exist_ok=True)
(PROJECT / "Cargo.toml").write_text(
    '[package]\nname = "renamed-consumer"\nversion = "0.0.0"\n'
    'edition = "2021"\nrust-version = "1.80"\npublish = false\n\n'
    '[workspace]\n\n[dependencies]\n'
    f'instrumentation = {{ package = "spanscope", path = "{ROOT / "spanscope"}", '
    'default-features = false, features = ["enabled"] }\n'
)
(SOURCE / "main.rs").write_text(
    '#[instrumentation::trace(root, name = "renamed::entry")]\n'
    'fn entry() { let _guard = instrumentation::span!("renamed::inner"); }\n'
    'fn main() { entry(); let snapshot = instrumentation::collection::snapshot(); '
    'assert_eq!(snapshot.roots.len(), 1); }\n'
)
bin_directory = SOURCE / "bin"
bin_directory.mkdir(exist_ok=True)
(bin_directory / "guard_send.rs").write_text(
    'fn main() { let guard = instrumentation::span!("thread-bound"); '
    'std::thread::spawn(move || drop(guard)); }\n'
)
shutil.copyfile(ROOT / "Cargo.lock", PROJECT / "Cargo.lock")
environment = os.environ.copy()
environment["CARGO_TARGET_DIR"] = str(ROOT / "target")
for toolchain in ("stable", "1.80.0"):
    subprocess.run(
        ["cargo", f"+{toolchain}", "run", "--offline", "--bin", "renamed-consumer",
         "--manifest-path", str(PROJECT / "Cargo.toml")],
        check=True,
        env=environment,
    )
    guard_check = subprocess.run(
        ["cargo", f"+{toolchain}", "check", "--offline", "--bin", "guard_send",
         "--manifest-path", str(PROJECT / "Cargo.toml")],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=environment,
    )
    assert guard_check.returncode != 0, "span guard unexpectedly implements Send"
    assert "cannot be sent between threads safely" in guard_check.stdout, guard_check.stdout
    print(f"renamed-dependency consumer passed on {toolchain}")
