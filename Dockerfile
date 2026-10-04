# Multi-stage build for Paqtra CLI + agent binary
FROM docker.io/library/rust:bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY web-api/paqtra-metrics ./web-api/paqtra-metrics
# The Helm chart is compiled into the binary (src/cli/chart.rs).
COPY chart ./chart
RUN cargo build --release

FROM docker.io/library/debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 wget \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 1000 paqtra
COPY --from=builder /build/target/release/paqtra /usr/local/bin/paqtra
USER paqtra
EXPOSE 9192
ENTRYPOINT ["paqtra"]
CMD ["agent"]
