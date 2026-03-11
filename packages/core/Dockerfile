FROM rust:1.81-slim AS builder
WORKDIR /build

# Copy workspace manifest first for layer caching
COPY Cargo.toml Cargo.lock* ./
COPY packages/core packages/core
COPY contracts contracts

RUN cargo build --release -p nerve-core

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/nerve-core /usr/local/bin/nerve-core
EXPOSE 8080
CMD ["nerve-core"]
