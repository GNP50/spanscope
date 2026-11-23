## What this changes

<!-- A short description of the change and why it is needed. Link the issue it addresses (Fixes #123). -->

## How it was tested

<!-- Tests added or updated, manual checks of the report (browsers), overhead numbers for runtime work. -->

## Checklist

- [ ] `cargo test --workspace --all-features --locked` passes locally
- [ ] `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` pass
- [ ] The scripts in `scripts/` pass (`check_contract.py`, `check_packages.py`, `check_features.py`)
- [ ] Viewer changes: `npm test` and `npm run test:e2e` pass in `viewer/`, and the rebuilt `viewer.html` copies are committed (`npm run verify:asset`)
- [ ] Tests cover the change
- [ ] The READMEs are updated if behaviour, a feature flag or the CLI changed
