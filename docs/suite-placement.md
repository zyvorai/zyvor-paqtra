# Suite placement (Cilium, Paqtra, Netra)

Paqtra is the **Cilium-native** observe/ops sibling. **Netra** is the independent eBPF sibling (own programs and maps under `/sys/fs/bpf/netra`, leased emergency control). They must not fight.

| Role | Owns | Must not |
|------|------|----------|
| **PacketWolf** | Commercial superset of Paqtra: kernel attribution, threat detection, containment, operator | See its own license and docs |
| **Cilium** | CNI, policy maps, identity, datapath (`cil_*`) | — |
| **Paqtra** | Hubble/API/UI, install/status CLI, **read-only** map + program inventory | Write Cilium maps; attach/replace Cilium programs; second CNI |
| **Netra** | Own programs under `/sys/fs/bpf/netra`, TCX/XDP, netlink | Modify Cilium maps |

| Choose **Paqtra** when… | Choose **Netra** when… |
| --- | --- |
| Cilium is already the CNI of record | You need CNI-independent observe on cgroup v2 alone |
| You want Hubble flows, path investigation and policy preview in one place | You want a leased emergency deny with automatic return to observe |
| Policy changes should stay in Cilium CNPs | You need kernel drop attribution and packet capture without a CNI |

Full rules: [docs/cilium-brotherhood.md](cilium-brotherhood.md). Agent instructions: [AGENTS.md](../AGENTS.md).
