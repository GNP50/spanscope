#!/usr/bin/env python3
"""Check package inventories and license copies; not a publish dry run."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
asset = (ROOT / "cargo-spanscope/assets/viewer.html").read_bytes()
assert asset == (ROOT / "spanscope/assets/viewer.html").read_bytes(), "viewer asset copies differ"
assert len(asset) <= 3_000_000, f"viewer HTML is {len(asset)} bytes (3 MB target)"
assert b"__SPANSCOPE_BOOTSTRAP__" in asset, "viewer template has no bootstrap slot"
print(f"viewer asset: {len(asset)} bytes, both crate copies identical")
for package in ("spanscope-macros", "spanscope", "cargo-spanscope"):
    for name in ("LICENSE-MIT", "LICENSE-APACHE"):
        assert (ROOT / name).read_bytes() == (ROOT / package / name).read_bytes(), (package, name)
    listing = subprocess.check_output([
        "cargo", "package", "--locked", "--list", "--allow-dirty", "-p", package,
    ], cwd=ROOT, text=True).splitlines()
    for required in ("Cargo.toml", "README.md", "LICENSE-MIT", "LICENSE-APACHE"):
        assert required in listing, (package, required)
    forbidden = ("node_modules/", "viewer/src/", ".github/", "target/")
    assert not any(any(part in path for part in forbidden) for path in listing), (package, listing)
    assert "build.rs" not in listing, (package, "unexpected build script")
    print(f"{package}: {len(listing)} package entries, licenses identical, no build script or development tree")
