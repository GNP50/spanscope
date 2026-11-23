# Contributing to spanscope

Thanks for taking the time to help. Bug reports, fixes, documentation, new diagnostics and
report views are all welcome. For a larger change (the profile schema, the macro syntax, a
new feature flag or a new report view), please open an issue first so the approach can be
agreed on before you spend time on it.

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md). Security
problems go through [private reporting](SECURITY.md), not public issues.

## Toolchain

- **Rust 1.80 or newer** (the `rust-version` in `Cargo.toml`); CI tests on stable and 1.80.0,
  on Linux, macOS and Windows. Lint runs on stable with `rustfmt` and `clippy`.
- **Python 3** for the scripts in `scripts/` (CI uses 3.12). `check_contract.py` needs the
  packages in [scripts/requirements.txt](scripts/requirements.txt):

  ```sh
  python3 -m pip install -r scripts/requirements.txt
  ```

- **For the viewer only**: Node.js 22 and npm, plus Rust with the WebAssembly target and
  `wasm-bindgen-cli` 0.2.100, because `npm ci` builds the pinned `skeinmap` dependency:

  ```sh
  rustup target add wasm32-unknown-unknown
  cargo install wasm-bindgen-cli --version 0.2.100 --locked
  ```

  Browser tests use Chromium through Playwright (`npx playwright install chromium` in
  `viewer/`).

## Repository layout

| Path                | What lives there                                                                                    |
| ------------------- | --------------------------------------------------------------------------------------------------- |
| `spanscope/`        | The runtime crate: collection, export, feature flags ([guide](spanscope/README.md)).                |
| `spanscope-macros/` | The `#[trace]` and `#[derive(Tracked)]` procedural macros.                                          |
| `cargo-spanscope/`  | The report CLI ([guide](cargo-spanscope/README.md)).                                                |
| `viewer/`           | The source of the single-file HTML report ([guide](viewer/README.md)).                              |
| `schema/`           | The profile JSON schema (`profile-v1.schema.json`).                                                 |
| `scripts/`          | Contract, feature, package and expansion checks run by CI, and a benchmark helper.                  |

## Checks

The Rust side (see also "Develop and verify" in the [README](README.md)):

```sh
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS=-D\ warnings cargo doc --workspace --no-deps --all-features --locked
python3 scripts/check_features.py
python3 scripts/check_renamed_dependency.py
python3 scripts/check_contract.py
python3 scripts/check_packages.py
```

CI also runs `python scripts/check_disabled_expansion.py`, which needs
`cargo install cargo-expand --version 1.0.118 --locked`.

The viewer:

```sh
npm ci --prefix viewer
npm run build --prefix viewer
npm run verify:asset --prefix viewer
npm test --prefix viewer
cargo build -p cargo-spanscope --locked
npm run test:e2e --prefix viewer
```

`npm run build` writes the same `viewer.html` into both crates. If you change the viewer,
commit the rebuilt copies; `npm run verify:asset` fails when they are out of date. A change
to the production dependencies also changes
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) and its copies in both crates, which
`npm run build` refreshes.

The workspace denies `missing_docs` and `unsafe_code`: every public item needs rustdoc, and
there is no `unsafe`.

## Pull requests

- Keep each pull request focused on one change; add or update tests with it. A bug fix comes
  with a test that reproduces the bug.
- Run the checks for the parts you touched before pushing.
- The profile schema is derived from the Rust types. After changing them, refresh
  `schema/profile-v1.schema.json` with `python3 scripts/check_contract.py --write` (CI fails
  on drift); `npm run build --prefix viewer` refreshes the viewer's types from the schema.
- Update the READMEs when you change behaviour, a feature flag or the CLI.
- Use [conventional commits](https://www.conventionalcommits.org/) for commit messages and
  pull request titles (`feat`, `fix`, `docs`, `build`, `chore`, `test`, `refactor`).

## License of contributions

spanscope is dual licensed under MIT or Apache-2.0, at the user's option. Unless you
explicitly state otherwise, any contribution intentionally submitted for inclusion in this
project by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
