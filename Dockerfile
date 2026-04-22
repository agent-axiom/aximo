FROM rust:1.88-bookworm AS builder

WORKDIR /app

COPY . .

RUN cargo build --release -p aximo --locked

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 aximo \
    && mkdir -p /app/config /var/lib/aximo/models \
    && chown -R aximo:aximo /app /var/lib/aximo

COPY --from=builder /app/target/release/aximo /usr/local/bin/aximo

WORKDIR /app
ENV AXIMO_CONFIG=/app/config/aximo.toml

USER aximo

EXPOSE 8080

CMD ["aximo"]
