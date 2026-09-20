# Security Policy

## Supported versions

spanscope is at `0.1.x`. Only the latest release receives fixes.

## Reporting a vulnerability

Please do **not** open a public issue for a security problem.

Use GitHub's private vulnerability reporting instead:
[Report a vulnerability](https://github.com/GNP50/spanscope/security/advisories/new).

This opens a private advisory visible only to the maintainer. Expect an
acknowledgement within a few days; this is a spare-time project, so please
allow reasonable time for a fix before any public disclosure.

## Scope

spanscope performs no network I/O and sends no telemetry. A profile is written
to the path the host application configures, and nothing leaves the machine.

The parts worth scrutiny are:

- `spanscope/src/allocation.rs` — the optional `GlobalAlloc` wrapper behind the
  `alloc-tracker` feature, the only `unsafe` runtime module.
- `spanscope/src/export.rs` — profile serialisation and the `libc::atexit` flush.
- `cargo-spanscope` — report generation, which writes HTML to a directory you
  pass and may invoke the platform file opener on it.
- `assets/viewer.html` — the offline report. It parses a profile as data; it
  does not execute it, and it makes no network requests.

Reports about a profile parsed from an untrusted source causing the viewer or
the CLI to misbehave are in scope and welcome.
