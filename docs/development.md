# Development and CI

See [installation](installation.md) for the pinned Rust toolchain and native prerequisites. No models are needed for the unit/integration suite: it uses fake engines for API tests. Real model inference is a separate smoke check.

## Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
```

Optional helpers: `just fmt` formats files, `just lint` runs Clippy, and `just test` runs workspace tests. Python 3 runs the source-setup script tests and docs-link check:

```bash
python3 -m unittest discover -s scripts/tests -v
python3 scripts/check-doc-links.py
```

Coverage uses `cargo install cargo-llvm-cov --locked` and the `llvm-tools-preview` Rust component:

```bash
cargo llvm-cov --workspace --locked --json --summary-only \
  --output-path coverage-summary.json --fail-under-lines 88
```

## What the badges mean

The README badges explicitly track `main`, not a feature branch. A PR passing does not turn a failing `main` badge green until its fix is merged and the relevant main workflow succeeds.

- **CI:** formatting, Clippy, tests, and the 88% line-coverage floor
- **Security:** `cargo audit --deny warnings`, `cargo deny check`, and CycloneDX SBOM generation
- **Image Security:** Trivy and Grype scan a freshly built image, failing on fixable high/critical findings
- **Container:** publishes `ghcr.io/agent-axiom/aximo` on main pushes, semantic version tags, or manual dispatch; model files are excluded, and BuildKit requests SBOM/provenance attestations
- **Coverage:** the last measured main-branch percentage in `badges/coverage.json`, updated by CI; it is not a live measure of an unmerged PR

Keep security checks strict. Fix vulnerable or yanked dependencies rather than hiding failed jobs or adding advisory exceptions merely to change a badge. Dependency updates should preserve the lockfile and be checked with the commands above.

## Library packaging

`aximo-core`, `aximo-audio`, and `aximo-inference` are publishable library crates. The `aximo` service crate has `publish = false` but can be installed from its checkout.

`just package-libs` checks `aximo-core` and `aximo-audio`. Dry-run publishing `aximo-inference` requires `aximo-core` to already be available in the crates.io index. See [publishing](publishing.md).
