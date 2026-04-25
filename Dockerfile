# ── Build stage ───────────────────────────────────────────────────────────────
FROM rust:1.80-bookworm AS builder

WORKDIR /build

# Cache dependencies first
COPY Cargo.toml Cargo.lock ./
COPY crates/cli/Cargo.toml        crates/cli/
COPY crates/config/Cargo.toml     crates/config/
COPY crates/core/Cargo.toml       crates/core/
COPY crates/minecraft/Cargo.toml  crates/minecraft/
COPY crates/llm/Cargo.toml        crates/llm/
COPY crates/commands/Cargo.toml   crates/commands/
COPY crates/server/Cargo.toml     crates/server/

# Stub src/lib.rs so cargo fetch works
RUN for c in config core minecraft llm commands server; do \
      mkdir -p crates/$c/src && echo "pub fn _stub() {}" > crates/$c/src/lib.rs; \
    done && \
    mkdir -p crates/cli/src && echo "fn main() {}" > crates/cli/src/main.rs

RUN cargo build --release 2>/dev/null || true

# Full build
COPY . .
RUN cargo build --release --bin mindcraft

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/mindcraft /usr/local/bin/mindcraft

# Profiles, tasks, and web UI
COPY profiles/  profiles/
COPY src/mindcraft/public/ src/mindcraft/public/

# Config files (mount these as volumes in prod)
# keys.json and settings.json are expected at /app/

EXPOSE 8080

CMD ["mindcraft", "--settings", "settings.json", "--keys", "keys.json"]
