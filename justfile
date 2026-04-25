default:
    @just --list

test:
    cargo xtest

fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

coverage:
    cargo llvm-cov --workspace --summary-only

setup-models:
    ./scripts/fetch-models.sh

benchmark-api:
    ./scripts/benchmark-api.sh

benchmark-fixtures:
    ./scripts/generate-benchmark-fixtures.sh

benchmark-report:
    ./scripts/render-benchmark-report.sh

package-libs:
    cargo package -p aximo-core --allow-dirty
    cargo package -p aximo-audio --allow-dirty
