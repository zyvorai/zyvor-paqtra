import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import {CtaBand, HeroShell, HonestyBand, Page} from '@site/src/components/marketing/Shell';
import {
  Agents,
  Explorer,
  F,
  Quickstart,
  Receipts,
  Shipped,
  StackScene,
  Walkthrough,
  Why,
} from '@site/src/components/home';
import s from '@site/src/components/home/Home.module.css';

export default function Home(): ReactNode {
  return (
    <Layout
      title="Paqtra — Cilium-native network observability and flow investigation"
      description="See where network traffic goes and why it is allowed or dropped. Web console, REST API, terminal UI and a read-only MCP server for Kubernetes clusters running Cilium.">
      <Page>
        <HeroShell
          eyebrow="Cilium-native network observability"
          title="Trace every flow."
          accent="Know why it dropped."
          sub="Hubble flows, policy verdicts and per-second metrics in one console, with path investigation that explains why workload A cannot reach B. Paqtra observes; Cilium decides."
          buttons={
            <>
              <Link className={`${s.btn} ${s.btnPrimary}`} to="/docs/getting-started/quickstart">
                Get started
              </Link>
              <a className={`${s.btn} ${s.btnGhost}`} href="#product">
                See the product
              </a>
              <Link className={`${s.btn} ${s.btnGhost}`} to="https://github.com/zyvorai/zyvor-paqtra">
                GitHub
              </Link>
            </>
          }
          stats={[
            [String(F.mcpTools), 'MCP tools, all read-only'],
            [String(F.builtinAlerts), 'built-in metric alerts'],
            [String(F.apiPaths), 'documented REST API paths'],
            ['Apache 2.0', 'free community edition'],
          ]}
          cueHref="#why"
          cueLabel="Scroll to the next section"
        />
        <Why />
        <StackScene />
        <Walkthrough />
        <Explorer />
        <Agents />
        <Receipts />
        <Shipped />
        <Quickstart />
        <HonestyBand
          items={[
            <>Paqtra is read-only toward the datapath: it does not attach programs or write Cilium maps.</>,
            <>It targets clusters that already run Cilium with Hubble; it is not a CNI.</>,
            <>Kernel process attribution, threat detection, gated containment and an operator are PacketWolf features, not Paqtra.</>,
            <>Baseline anomaly scoring is statistical, not a threat detector.</>,
          ]}
        />
        <CtaBand title="Need more than visibility?">
          <Link className={`${s.btn} ${s.btnPrimary}`} to="/docs/getting-started/quickstart">
            Quickstart
          </Link>
          <Link className={`${s.btn} ${s.btnGhost}`} to="/docs/paqtra-vs-packetwolf">
            Compare with PacketWolf
          </Link>
        </CtaBand>
      </Page>
    </Layout>
  );
}
