# Publishing Library Crates

`aximo` is a service binary and is not published to `crates.io`.

The workspace is prepared so these crates can be published independently:

1. `aximo-core`
2. `aximo-audio`
3. `aximo-inference`

Publish in that order because `aximo-inference` depends on `aximo-core`.

## Dry Run

```bash
just package-libs
```

This fully verifies the crates that do not depend on unpublished internal workspace packages:

- `aximo-core`
- `aximo-audio`

`aximo-inference` cannot be packaged or dry-run published until `aximo-core` is already visible in the `crates.io` index.

## Release Steps

1. Update `version` in the workspace root if you are cutting a new release.
2. Run formatting, lint, tests, coverage, and packaging checks.
3. Publish `aximo-core`.
4. Publish `aximo-audio`.
5. Publish `aximo-inference`.
6. Wait for each dependency crate to appear on `crates.io` before publishing the dependent crate.

## Commands

```bash
cargo publish -p aximo-core
cargo publish -p aximo-audio
cargo publish -p aximo-inference
```

## Notes

- Internal workspace dependencies already carry both `path` and `version` metadata, which is required for publishable crates.
- Keep model weights out of published crates; they remain runtime artifacts configured by path.
- If crate-level documentation becomes important for public consumers, add dedicated crate READMEs before the first public release.
- Before publishing `aximo-inference`, rerun `cargo publish -p aximo-inference --dry-run` after `aximo-core` is available in the index.
