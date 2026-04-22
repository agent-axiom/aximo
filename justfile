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

package-libs:
    cargo package -p aximo-core --allow-dirty
    cargo package -p aximo-audio --allow-dirty
