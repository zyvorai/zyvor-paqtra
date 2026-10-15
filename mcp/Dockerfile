# paqtra-mcp: the stdio MCP server, packaged for the MCP Registry (server.json).
# Build from the repository root:
#   docker build -f mcp/Dockerfile -t ghcr.io/zyvorai/paqtra-mcp .
FROM docker.io/library/rust:bookworm AS builder
WORKDIR /build
COPY mcp/Cargo.toml mcp/Cargo.lock ./
COPY mcp/src ./src
RUN cargo build --release --locked

FROM gcr.io/distroless/cc-debian12:nonroot
# The MCP Registry verifies ownership of an OCI package by this label; it must
# equal "name" in server.json (the release version gate checks that).
LABEL io.modelcontextprotocol.server.name="io.github.zyvorai/paqtra"
LABEL org.opencontainers.image.source="https://github.com/zyvorai/paqtra"
LABEL org.opencontainers.image.licenses="Apache-2.0"
COPY --from=builder /build/target/release/paqtra-mcp /paqtra-mcp
ENTRYPOINT ["/paqtra-mcp"]
