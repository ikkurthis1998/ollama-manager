FROM rust:1.74-slim AS builder

WORKDIR /app

# Install OpenSSL development packages
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Create a new empty shell project
RUN cargo new --bin ollama-manager
WORKDIR /app/ollama-manager

# Copy over manifests
COPY ./Cargo.* ./

# Cache dependencies
RUN cargo build --release
RUN rm src/*.rs

# Copy source code
COPY ./src ./src

# Build for release
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim AS runtime

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create unprivileged user
RUN useradd -m -u 1000 -U app

WORKDIR /app

# Copy only the binary and config
COPY --from=builder /app/ollama-manager/target/release/ollama-manager .
COPY --chown=app:app ./config /app/config

# Set file permissions
RUN chown app:app /app/ollama-manager && \
    chmod +x /app/ollama-manager

# Switch to unprivileged user
USER app

EXPOSE 3000

CMD ["./ollama-manager"]
