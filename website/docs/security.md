---
title: Security
sidebar_position: 4
---

Paqtra is observe-first and read-only toward the datapath. This page covers the boundaries it keeps and the controls on the web stack.

## Boundaries

These are hard rules, repeated in [AGENTS.md](https://github.com/zyvorai/zyvor-paqtra/blob/main/AGENTS.md) and explained in [Cilium boundary](core-concepts/cilium-brotherhood.md).

- Never writes Cilium BPF maps or pins over `cil_*` programs.
- Never attaches, detaches or replaces Cilium (or Netra) programs. Attachment inventory is read-only classification (`cil_*` / `netra_*` / other).
- The agent observes, reports health and inventories. Policy apply goes through Cilium CRDs.
- Does not collect application payloads, argv/cmdline, or Secret contents.
- Does not introduce a second CNI or compete with Cilium's datapath.
- Prefers Hubble for flows; uses maps for node-local enrichment only.

## Web stack

- JWT authentication (HS256, 32+ character secret). `ADMIN_PASSWORD` is required when auth is enabled; there are no default credentials.
- RBAC-ready middleware.
- Input validation on all kubectl-bound fields.
- CORS with an explicit origin allowlist.
- Rate limiting.
- Confirmation dialogs on destructive operations.
- Secure temporary files via `tempfile::NamedTempFile`.
- `AUTH_DISABLED=true` is for development only and requires `ENVIRONMENT=development` or `test`.

## Reporting a vulnerability

Use [GitHub Security Advisories](https://github.com/zyvorai/zyvor-paqtra/security/advisories/new) for the repository. Please don't open a public issue with exploit details before maintainers have had a chance to respond. See [SECURITY.md](https://github.com/zyvorai/zyvor-paqtra/blob/main/SECURITY.md).

## Further reading

- [Security whitepaper](https://github.com/zyvorai/zyvor-paqtra/blob/main/docs/client/security-whitepaper.html)
- [Web architecture](https://github.com/zyvorai/zyvor-paqtra/blob/main/docs/web-architecture.md)
