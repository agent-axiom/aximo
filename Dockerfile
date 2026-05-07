# syntax=docker/dockerfile:1.7

# ort/onnxruntime prebuilt binaries used by transcribe-rs on aarch64 require
# newer glibc/libstdc++ symbols than Debian bookworm provides.
FROM rust:1.95.0-trixie AS builder

WORKDIR /app

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git/db,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release -p aximo --locked \
    && cp /app/target/release/aximo /usr/local/bin/aximo

FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 aximo \
    && mkdir -p /app/config /var/lib/aximo/models \
    && chown -R aximo:aximo /app /var/lib/aximo

COPY --from=builder /usr/local/bin/aximo /usr/local/bin/aximo

WORKDIR /app
ENV AXIMO_CONFIG=/app/config/aximo.toml
ENV ORT_LOG=error

USER aximo

EXPOSE 8080

CMD ["aximo"]
