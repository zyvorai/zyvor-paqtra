// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import type {ReactNode} from 'react';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import useBaseUrl from '@docusaurus/useBaseUrl';
import styles from './gallery.module.css';

type Shot = {
  src: string;
  caption: string;
};

const TOUR: Shot[] = [
  {src: '/00-overview.png', caption: 'Overview'},
  {src: '/01-flows.png', caption: 'Flows: Hubble verdicts'},
  {src: '/02-investigate.png', caption: 'Path investigation'},
  {src: '/03-drops.png', caption: 'Drops by Cilium reason'},
  {src: '/04-service-map.png', caption: 'Service map'},
  {src: '/05-topology.png', caption: 'Topology'},
  {src: '/06-policies.png', caption: 'Policies'},
  {src: '/07-ebpf.png', caption: 'eBPF inventory (read-only)'},
  {src: '/08-diagnostics.png', caption: 'Diagnostics'},
];

function ShotCard({shot}: {shot: Shot}) {
  const src = useBaseUrl(shot.src);
  return (
    <figure className={styles.shot}>
      <img src={src} alt={shot.caption} loading="lazy" />
      <figcaption>{shot.caption}</figcaption>
    </figure>
  );
}

export default function Gallery(): ReactNode {
  return (
    <Layout
      title="Gallery"
      description="A walkthrough of the Paqtra dashboard, captured against a live lab cluster.">
      <header className={styles.header}>
        <div className="container">
          <Heading as="h1">Product tour</Heading>
          <p>
            Every screenshot below is captured against a running lab cluster
            with Cilium and Hubble, not a mockup.
          </p>
        </div>
      </header>
      <main className="container">
        <div className={styles.grid}>
          {TOUR.map((shot) => (
            <ShotCard key={shot.src} shot={shot} />
          ))}
        </div>
      </main>
    </Layout>
  );
}
