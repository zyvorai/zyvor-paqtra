import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import FeatureHighlights from '@site/src/components/FeatureHighlights';
import ScreenshotStrip from '@site/src/components/ScreenshotStrip';
import Reveal from '@site/src/components/Reveal';

import styles from './index.module.css';

function HomepageHeader() {
  const hero = useBaseUrl('/paqtra-live-demo.gif');
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <div className="container">
        <div className={styles.heroGrid}>
          <div>
            <Heading as="h1" className="hero__title">
              Trace every flow.
            </Heading>
            <p className="hero__subtitle">
              See where network traffic goes and why it is allowed or dropped.
              Web dashboard, REST API and terminal TUI for Kubernetes clusters
              running Cilium. Paqtra observes; Cilium decides.
            </p>
            <div className={styles.buttons}>
              <Link
                className="button button--secondary button--lg"
                to="/docs/getting-started/quickstart">
                Get Started
              </Link>
              <Link
                className="button button--outline button--lg button--secondary"
                to="/gallery">
                Product tour
              </Link>
              <Link
                className="button button--outline button--lg button--secondary"
                to="https://github.com/zyvorai/zyvor-paqtra">
                View on GitHub
              </Link>
            </div>
          </div>
          <div className={styles.heroMedia}>
            <img src={hero} alt="Paqtra live demo — Overview dashboard, live datapath and flow chapters" />
            <p className={styles.heroMediaCaption}>
              Captured against a live lab cluster, not a mockup.
            </p>
          </div>
        </div>
      </div>
    </header>
  );
}

function ProblemStatement() {
  return (
    <section className={styles.problem}>
      <div className="container">
        <Reveal className="row">
          <div className="col col--8 col--offset-2 text--center">
            <Heading as="h2" className={styles.sectionHeading}>
              Why Cilium-native?
            </Heading>
            <p>
              If Cilium is already your CNI, the flows, identities and policy
              verdicts you need are already there. Paqtra reads them through
              Hubble and the Kubernetes API, and adds node-local enrichment
              from the BPF map inventory. It shows you the path, the drop
              reason and the policy that caused it in one place.
            </p>
            <p>
              Paqtra never competes with Cilium&apos;s datapath. It does not
              write Cilium BPF maps, attach or replace Cilium programs, or act
              as a second CNI. Policy changes go through{' '}
              <code>CiliumNetworkPolicy</code> objects, and Cilium enforces
              them. Paqtra doesn&apos;t collect application payloads, argv or
              Secret contents.
            </p>
            <Link to="/docs/core-concepts/cilium-brotherhood">
              Read the Cilium boundary →
            </Link>
          </div>
        </Reveal>
      </div>
    </section>
  );
}

function TrustBand() {
  return (
    <section className={styles.trust}>
      <div className="container">
        <Reveal className={styles.trustGrid}>
          <div>
            <Heading as="h3" className={styles.sectionHeading}>
              Open source, and honest about its limits
            </Heading>
            <p>
              Licensed under Apache 2.0. CI runs Rust format, clippy and tests,
              the web API and UI builds, and the chart and CLI smoke scripts.
              Paqtra is observe-first by design, and its boundaries are written
              down and enforced in review.
            </p>
            <Link to="/docs/security">Read the security notes →</Link>
          </div>
          <div className={styles.trustBadges}>
            <img
              src="https://github.com/zyvorai/zyvor-paqtra/actions/workflows/ci.yml/badge.svg"
              alt="CI status"
            />
            <img
              src="https://img.shields.io/badge/license-Apache%202.0-blue.svg"
              alt="Apache 2.0 license"
            />
          </div>
        </Reveal>
      </div>
    </section>
  );
}

function PacketWolfBand() {
  return (
    <section className={styles.problem}>
      <div className="container">
        <Reveal className="row">
          <div className="col col--8 col--offset-2 text--center">
            <Heading as="h2" className={styles.sectionHeading}>
              Need more than visibility?
            </Heading>
            <p>
              Paqtra is the free community edition of{' '}
              <strong>PacketWolf</strong>, Zyvor&apos;s commercial Cilium
              platform. PacketWolf adds kernel process attribution, threat
              detection, gated containment with rollback, a Kubernetes
              operator, an AI network copilot, enterprise sign-in and
              consoles for VMs and standalone containers.
            </p>
            <Link
              className="button button--primary button--lg"
              to="/docs/paqtra-vs-packetwolf">
              Compare Paqtra and PacketWolf
            </Link>
          </div>
        </Reveal>
      </div>
    </section>
  );
}

function GetStarted() {
  return (
    <section className={styles.enterprise}>
      <div className="container text--center">
        <Reveal>
          <Heading as="h2" className={styles.sectionHeading}>
            Try it on your cluster
          </Heading>
          <p className={styles.enterpriseCopy}>
            You need a Kubernetes cluster with Cilium and Hubble enabled.
            Docker Compose, Helm and a remote K3s script are covered in the
            quickstart.
          </p>
          <Link
            className="button button--primary button--lg"
            to="/docs/getting-started/quickstart">
            Read the quickstart
          </Link>
        </Reveal>
      </div>
    </section>
  );
}

export default function Home(): ReactNode {
  return (
    <Layout
      title="Paqtra — Cilium-native network observability and operations"
      description="See where network traffic goes and why it is allowed or dropped. Web dashboard, REST API and TUI for Kubernetes clusters running Cilium.">
      <HomepageHeader />
      <main>
        <ProblemStatement />
        <Reveal>
          <FeatureHighlights />
        </Reveal>
        <Reveal>
          <ScreenshotStrip />
        </Reveal>
        <TrustBand />
        <PacketWolfBand />
        <GetStarted />
      </main>
    </Layout>
  );
}
