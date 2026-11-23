# Contributing

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).

1. Open an issue describing the behavior or change.
2. Keep the Cilium/Paqtra/Netra responsibility boundary intact: never write
   Cilium BPF maps, pin over `cil_*`, or attach/replace Cilium programs. See
   `docs/cilium-brotherhood.md` and `AGENTS.md`.
3. Add tests for TUI, API, or UI behavior that you change.
4. Run `make check-all` and `make test-all` before submitting a PR (same gates
   as CI for the Rust/UI stack).
5. Contributions are accepted under the Zyvor Production Licence v2.0. See `LICENSE`.

## Layout

```
paqtra/
├── src/           # TUI + agent CLI
├── web-api/       # Axum REST API
├── web-ui/        # React dashboard
├── chart/         # Helm chart
├── deployments/   # Compose + raw manifests
├── docs/          # Product docs (flat kebab-case)
├── examples/      # Sample configs and scenarios
└── tests/         # Integration tests
```

## Prerequisites

- Rust 1.75+
- Node.js 24 (what CI and the web-ui image use)
- Kubernetes cluster with Cilium 1.14+ (for end-to-end work)
