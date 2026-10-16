import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import Heading from '@theme/Heading';
import styles from './styles.module.css';

type FeatureItem = {
  title: string;
  description: ReactNode;
  to: string;
};

const FeatureList: FeatureItem[] = [
  {
    title: 'Flows with verdicts',
    description:
      'Live Hubble flows colored by Cilium’s verdict, with per-packet explanations and WebSocket streaming.',
    to: '/docs/getting-started/features',
  },
  {
    title: 'Why can’t A reach B?',
    description:
      'Evidence-backed path investigation. Every finding is labeled observed, inferred or unavailable.',
    to: '/docs/getting-started/features',
  },
  {
    title: 'Drops explained',
    description:
      'Packet drop analytics by Cilium reason, correlated with policy, with root-cause suggestions.',
    to: '/docs/getting-started/features',
  },
  {
    title: 'Policy, through Cilium',
    description:
      'Visual builder, YAML editor and AutoPolicy. Simulate first, then apply as CiliumNetworkPolicy. Cilium enforces.',
    to: '/docs/core-concepts/cilium-brotherhood',
  },
  {
    title: 'Read-only eBPF inventory',
    description:
      'Conntrack, policy maps, IP cache, LB maps and program attachments, classified as cil_*, netra_* or other. Never written.',
    to: '/docs/core-concepts/ebpf-integration',
  },
  {
    title: 'Web, API and TUI',
    description:
      'A React dashboard, an OpenAPI-documented REST API and a 13-tab terminal UI in one deployable package.',
    to: '/docs/core-concepts/architecture',
  },
  {
    title: 'Operations',
    description:
      'Diagnostics, packet capture, SLOs, alerts, audit log, chaos experiments and canary rollouts.',
    to: '/docs/getting-started/features',
  },
  {
    title: 'Multi-cluster',
    description:
      'Register clusters, check health, compare and sync policies, and view topology across the fleet.',
    to: '/docs/getting-started/features',
  },
  {
    title: 'Netra alongside',
    description:
      'Netra is the independent eBPF sibling with its own maps under /sys/fs/bpf/netra. The two never touch each other’s programs.',
    to: '/docs/core-concepts/cilium-brotherhood',
  },
];

function Feature({title, description, to}: FeatureItem) {
  return (
    <div className="col col--4">
      <Link to={to} className={styles.card}>
        <Heading as="h3">{title}</Heading>
        <p>{description}</p>
      </Link>
    </div>
  );
}

export default function FeatureHighlights(): ReactNode {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          {FeatureList.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
      </div>
    </section>
  );
}
