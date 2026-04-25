# Security Policy

## Supported Versions

Security fixes target the `main` branch until Aximo starts publishing versioned stable releases.

## Reporting a Vulnerability

Do not open a public issue for suspected vulnerabilities. Report privately through GitHub Security Advisories for this repository.

Please include:

- affected commit or release tag;
- reproduction steps;
- expected impact;
- whether model files, user audio, credentials, or deployment configuration are involved.

## Project Hardening

CI runs RustSec advisory checks, `cargo-deny`, and CycloneDX SBOM generation. Container releases also request BuildKit SBOM and provenance attestations.
