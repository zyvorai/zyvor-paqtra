# AGENTS.md

Instructions for coding agents working in this repository.

## What Paqtra is

Cilium-native network observability and operations for Kubernetes. It ships a
web dashboard, REST API, and terminal TUI. Flows prefer Hubble; node-local
enrichment may read BPF map inventory. Enforcement stays with Cilium
(`CiliumNetworkPolicy` / CNP).

Free, Apache-2.0 community edition of **PacketWolf** (Zyvor's commercial Cilium
platform; see `docs/paqtra-vs-packetwolf.md`). Suite sibling to **Netra**
(independent eBPF observe + leased emergency control). Co-existence rules:
`docs/cilium-brotherhood.md`.

## Hard boundaries

- Never write Cilium BPF maps or pin over `cil_*` programs.
- Never attach, detach, or replace Cilium (or Netra) programs. Attachment
  inventory is **read-only** classification (`cil_*` / `netra_*` / other).
- Agent = observe / health / inventory; policy apply goes through Cilium CRDs.
- Do not collect application payloads, argv/cmdline, or Secret contents.
- Do not introduce a second CNI or compete with Cilium’s datapath.
- Prefer Hubble for flows; maps for node-local enrichment only.
- Details: `docs/cilium-brotherhood.md`, `docs/ebpf-integration.md`.
- Metrics platform (`web-api/paqtra-metrics`, `src/metrics`,
  `/api/v1/metrics/*`): collectors are read-only; the process collector uses
  `comm` only, never `cmdline` or `environ`. App credentials come only from env
  vars named in config (`username_env` / `password_env` / `bearer_env`), never
  from Secrets read through the API. Metric alerts and anomaly endpoints
  notify only and are never wired to policy apply. Details: `docs/metrics.md`.

## Validation

Run the same gates contributors use before a PR:

```bash
make check-all
make test-all
make build-all
```

CI also exercises chart/CLI/remote smoke scripts under `scripts/ci-*.sh` when
those jobs are enabled.

## Docs

Flat product topics live under `docs/*.md` (kebab-case). Client HTML under
`docs/client/`. Start with `README.md`, `QUICKSTART.md`, `docs/features.md`,
`docs/architecture.md`, and `docs/cilium-brotherhood.md`.

## License

Contributions are accepted under the Apache License 2.0. See `LICENSE`.
