# Security Policy

## Reporting a vulnerability

Please report suspected vulnerabilities through **[GitHub Security Advisories](https://github.com/zyvorai/paqtra/security/advisories/new)** for this repository.

Do not open a public issue with exploit details before maintainers have had a chance to respond.

We aim to acknowledge reports within a few business days.

## Scope

Paqtra is observe/ops on top of Cilium. It must not write Cilium BPF maps,
replace Cilium programs, or act as a second CNI. See
[`docs/cilium-brotherhood.md`](docs/cilium-brotherhood.md).

## Further reading

- [`docs/client/security-whitepaper.html`](docs/client/security-whitepaper.html)
- [`docs/web-architecture.md`](docs/web-architecture.md)
- [`README.md`](README.md#security)
