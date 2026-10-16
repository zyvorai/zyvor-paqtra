import {useState} from 'react';
import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Reveal from '@site/src/components/Reveal';
import facts from '@site/src/data/facts.generated.json';
import {Section} from '@site/src/components/marketing/Shell';
import s from './Home.module.css';

export const F = facts;

/* ------------------------------------------------------------- shared */

function Code({children}: {children: string}): ReactNode {
  const [done, setDone] = useState(false);
  const copy = () => {
    try {
      void navigator.clipboard.writeText(children).then(() => {
        setDone(true);
        window.setTimeout(() => setDone(false), 1500);
      });
    } catch {
      /* clipboard unavailable: leave the text selectable */
    }
  };
  return (
    <pre className={s.code}>
      {children}
      <button type="button" className={s.copy} onClick={copy} aria-label="Copy to clipboard">
        {done ? 'Copied' : 'Copy'}
      </button>
    </pre>
  );
}

function Tabs<T extends {id: string; label: string}>({
  items,
  value,
  onChange,
  label,
}: {
  items: readonly T[];
  value: string;
  onChange: (id: string) => void;
  label: string;
}): ReactNode {
  return (
    <div className={s.tabs} role="tablist" aria-label={label}>
      {items.map((t) => (
        <button
          key={t.id}
          type="button"
          role="tab"
          aria-selected={t.id === value}
          className={clsx(s.tab, t.id === value && s.tabOn)}
          onClick={() => onChange(t.id)}>
          {t.label}
        </button>
      ))}
    </div>
  );
}

/* ---------------------------------------------------------------- why */

export function Why(): ReactNode {
  const cards: [string, string][] = [
    [
      'Cilium-native',
      'Flows, identities and policy verdicts come from Hubble and the Kubernetes API, with node-local enrichment from the BPF map inventory. Nothing extra sits in the data path.',
    ],
    [
      'Answers, not just flows',
      'Ask why workload A cannot reach B and get the evidence: flows, drops, the policy involved, a likely owner and the next action, with confidence on every claim.',
    ],
    [
      'Read-only by design',
      'Paqtra never writes Cilium maps, attaches or replaces programs, or acts as a second CNI. Policy changes are CiliumNetworkPolicy objects that Cilium enforces.',
    ],
  ];
  return (
    <Section
      id="why"
      eyebrow="Why Paqtra"
      title="Cilium decides. Paqtra explains."
      lede="If Cilium is already your CNI, the data you need is already there. Paqtra puts the path, the drop reason and the policy behind it in one place, without competing with the datapath.">
      <div className={s.why}>
        {cards.map(([t, p], i) => (
          <Reveal key={t} delay={i * 80}>
            <div className={s.whyCard}>
              <h3>{t}</h3>
              <p>{p}</p>
            </div>
          </Reveal>
        ))}
      </div>
    </Section>
  );
}

/* -------------------------------------------------------- stack scene */

export function StackScene(): ReactNode {
  const Box = ({x, y, w, h, t, sub, accent}: {x: number; y: number; w: number; h: number; t: string; sub: string; accent?: boolean}) => (
    <g>
      <rect x={x} y={y} width={w} height={h} rx="14" className={accent ? s.sceneBoxAccent : s.sceneBox} />
      <text x={x + 16} y={y + 28} className={s.sceneTitle}>{t}</text>
      <text x={x + 16} y={y + 50} className={s.sceneSub}>{sub}</text>
    </g>
  );
  const outs: [string, string][] = [
    ['Web console', 'dashboard and investigate'],
    ['paqtra CLI + TUI', 'install, status, ui'],
    [`MCP, ${F.mcpTools} tools`, 'all read-only'],
    ['Prometheus, OTLP', 'metric exporters'],
  ];
  return (
    <Section
      id="how"
      tint
      eyebrow="How it works"
      title="Cilium and Hubble in, answers out."
      lede="Paqtra reads what Cilium already knows, stores flow history and per-second metrics, and serves one API to every client.">
      <Reveal>
        <div className={s.scene}>
          <svg viewBox="0 0 920 330" role="img" aria-label="Cilium and Hubble feed the Paqtra API and store, which serves the web console, the CLI and TUI, MCP and metric exporters">
            <Box x={10} y={120} w={200} h={70} t="Cilium + Hubble" sub="flows, verdicts, CRDs" />
            <Box x={270} y={120} w={210} h={70} t="Node agents" sub="per-second metrics, read-only" accent />
            <Box x={540} y={120} w={150} h={70} t="Paqtra API" sub="REST, SQLite, WS" accent />
            {outs.map(([t, sub], i) => (
              <Box key={t} x={740} y={10 + i * 80} w={170} h={64} t={t} sub={sub} />
            ))}
            <path d="M210 155 H270" className={s.sceneLink} />
            <path d="M480 155 H540" className={s.sceneLink} />
            {outs.map(([t], i) => (
              <path key={t} d={`M690 155 C 715 155, 715 ${42 + i * 80}, 740 ${42 + i * 80}`} className={s.sceneLink} />
            ))}
          </svg>
          <p className={s.sceneNote}>
            Paqtra never writes Cilium BPF maps or attaches programs. Agents collect host and workload metrics read-only.
          </p>
        </div>
      </Reveal>
    </Section>
  );
}

/* --------------------------------------------------------- walkthrough */

const SHOTS = [
  {id: 'overview', label: 'Overview', src: '/00-overview.png', q: 'What is the cluster doing right now?', body: 'Health, flows, drops and findings on one page, captured from a live lab cluster.', to: '/docs/getting-started/quickstart', cta: 'Quickstart'},
  {id: 'flows', label: 'Flows', src: '/01-flows.png', q: 'Which flows were allowed or dropped?', body: 'Hubble verdicts per flow, with a Why denied? action on every dropped row.', to: '/docs/investigate/path-investigation', cta: 'Investigate'},
  {id: 'investigate', label: 'Investigate', src: '/02-investigate.png', q: 'Why can A not reach B?', body: 'A step timeline, the likely owner (policy, DNS, no backend, datapath) and an exportable evidence bundle.', to: '/docs/investigate/path-investigation', cta: 'Path investigation'},
  {id: 'drops', label: 'Drops', src: '/03-drops.png', q: 'Why is traffic being dropped?', body: 'Drops grouped by the reason Cilium reports.', to: '/docs/investigate/rootcause', cta: 'Root cause'},
  {id: 'map', label: 'Service map', src: '/04-service-map.png', q: 'Who talks to whom?', body: 'The observed dependency graph between workloads.', to: '/gallery', cta: 'Gallery'},
  {id: 'policies', label: 'Policies', src: '/06-policies.png', q: 'What policy applies here?', body: 'CNP and CCNP objects Paqtra can see, with a simulator for changes.', to: '/docs/investigate/simulator', cta: 'Simulator'},
  {id: 'ebpf', label: 'eBPF inventory', src: '/07-ebpf.png', q: 'What is attached to this node?', body: 'A read-only classification of BPF attachments as Cilium, Netra or other.', to: '/docs/core-concepts/ebpf-integration', cta: 'eBPF integration'},
] as const;

function Shot({src, alt}: {src: string; alt: string}): ReactNode {
  return <img className={s.shot} src={useBaseUrl(src)} alt={alt} loading="lazy" />;
}

export function Walkthrough(): ReactNode {
  const [id, setId] = useState<string>(SHOTS[0].id);
  const cur = SHOTS.find((x) => x.id === id) ?? SHOTS[0];
  return (
    <Section
      id="product"
      eyebrow="The product"
      title="A real console, captured live."
      lede="Every screenshot comes from a running lab cluster, not a mockup.">
      <Reveal>
        <Tabs items={SHOTS} value={id} onChange={setId} label="Console pages" />
        <div className={s.panel}>
          <Shot src={cur.src} alt={`Paqtra ${cur.label} page`} />
          <div className={s.panelText}>
            <h3>{cur.label}</h3>
            <p className={s.q}>{cur.q}</p>
            <p>{cur.body}</p>
            <Link to={cur.to}>{cur.cta} →</Link>
            <p style={{marginTop: '1rem'}}>
              <Link to="/gallery">See the full tour →</Link>
            </p>
          </div>
        </div>
      </Reveal>
    </Section>
  );
}

/* ------------------------------------------------------------ explorer */

const CAPS = [
  {id: 'why', label: 'Why denied', q: 'Why was this flow dropped?', body: 'Explain one flow: the identities involved, the CNP/CCNP candidates, the drop reason and a draft minimal allow, each tagged with confidence.', cmd: 'POST /api/v1/investigate/flow', to: '/docs/investigate/path-investigation', cta: 'Path investigation'},
  {id: 'path', label: 'Path', q: 'Why can checkout not reach payments?', body: 'Explain a path from A to B on a port. You get steps, a likely owner and next actions, and a redacted evidence bundle you can export or share.', cmd: `curl -s -X POST $PAQTRA_URL/api/v1/investigate/path \\\n  -H "Authorization: Bearer $PAQTRA_TOKEN" \\\n  -d '{"source":{"namespace":"shop","name":"checkout"},\n       "destination":{"namespace":"shop","name":"payments"},\n       "port":443,"protocol":"TCP"}'`, to: '/docs/investigate/path-investigation', cta: 'Path investigation'},
  {id: 'simulate', label: 'Simulate', q: 'What would this policy change do?', body: 'Preview a policy against indexed flows: would_allow, would_deny or unknown, with the uncertainty listed. Rollback is a Cilium CRD change, never a BPF attach.', cmd: 'POST /api/v1/policies/simulate', to: '/docs/investigate/simulator', cta: 'Simulator'},
  {id: 'metrics', label: 'Measure', q: 'What changed in the last hour?', body: `Per-second node and workload metrics, anomaly scoring and ${F.builtinAlerts} built-in alert rules, plus series derived from Hubble flows.`, cmd: 'GET /api/v1/metrics/anomalies', to: '/docs/operate/metrics', cta: 'Metrics platform'},
  {id: 'automate', label: 'Automate', q: 'Can my agent do this?', body: `paqtra-mcp exposes ${F.mcpTools} tools over stdio. All of them are reads; there is no write tool.`, cmd: `docker run -i --rm -e PAQTRA_URL -e PAQTRA_TOKEN ${F.mcpImage}`, to: '/docs/integrations/mcp-integration', cta: 'MCP integration'},
] as const;

export function Explorer(): ReactNode {
  const [id, setId] = useState<string>(CAPS[0].id);
  const cur = CAPS.find((x) => x.id === id) ?? CAPS[0];
  return (
    <Section
      id="capabilities"
      tint
      eyebrow="Capabilities"
      title="Pick the question you have."
      lede="Each answer is a call you can make today, backed by a page of docs.">
      <Reveal>
        <Tabs items={CAPS} value={id} onChange={setId} label="Capabilities" />
        <div className={clsx(s.panel, s.panelWide)}>
          <div className={s.panelText}>
            <h3>{cur.q}</h3>
            <p>{cur.body}</p>
            <Link to={cur.to}>{cur.cta} →</Link>
          </div>
          <Code>{cur.cmd}</Code>
        </div>
      </Reveal>
    </Section>
  );
}

/* ----------------------------------------------------------- agents/MCP */

export function Agents(): ReactNode {
  const cfg = `{
  "mcpServers": {
    "paqtra": {
      "command": "docker",
      "args": ["run", "-i", "--rm",
               "-e", "PAQTRA_URL", "-e", "PAQTRA_TOKEN",
               "${F.mcpImage}"],
      "env": { "PAQTRA_URL": "https://paqtra.example:9191", "PAQTRA_TOKEN": "..." }
    }
  }
}`;
  return (
    <Section
      id="agents"
      eyebrow="For AI agents"
      title="Cilium visibility your agent can use."
      lede={`Paqtra is listed in the official MCP Registry as ${F.mcpName}.`}>
      <Reveal>
        <div className={s.numbers}>
          <div><b>{F.mcpTools}</b><span>tools</span></div>
          <div><b>0</b><span>write tools</span></div>
          <div><b>{F.apiPaths}</b><span>documented API paths</span></div>
          <div><b>{F.builtinAlerts}</b><span>built-in metric alerts</span></div>
        </div>
        <div className={s.agents}>
          <div className={s.panelText}>
            <p>
              Ask for the flows between two workloads, the drops in a namespace or an investigation bundle, and get structured answers from the live API. A read-only role is enough.
            </p>
            <div className={s.gate}>
              <b>Read-only end to end.</b> Every tool carries <code>readOnlyHint</code>, and the catalog generator fails the docs build if one does not.
            </div>
            <p style={{marginTop: '1rem'}}>
              <Link to="/docs/reference/mcp-tools">Browse the tool catalog →</Link>
              {'  '}
              <Link to="/docs/integrations/mcp-integration">Setup →</Link>
            </p>
          </div>
          <Code>{cfg}</Code>
        </div>
      </Reveal>
    </Section>
  );
}

/* ------------------------------------------------------------ receipts */

export function Receipts(): ReactNode {
  const items: [string, string][] = [
    ['Tested on every push', 'Rust format, clippy and tests, the web API and UI builds, and the chart, CLI and install-script smoke scripts.'],
    ['Read-only where it matters', 'No payload, argv or Secret collection. Paqtra never modifies Cilium maps or programs, and the process collector reads comm only.'],
    ['Signed and listed', 'Release binaries ship with sha256sums, a cosign keyless signature, an SBOM and a Homebrew formula, and the MCP server is in the official MCP Registry.'],
  ];
  return (
    <Section
      id="receipts"
      eyebrow="Receipts"
      title="Boring in the right places."
      lede="The guarantees are written down and checked in CI, not just promised.">
      <div className={s.cards3}>
        {items.map(([t, p], i) => (
          <Reveal key={t} delay={i * 80}>
            <div className={s.receipt}>
              <h3>{t}</h3>
              <p>{p}</p>
            </div>
          </Reveal>
        ))}
      </div>
      <div className={s.badges}>
        <img src="https://github.com/zyvorai/zyvor-paqtra/actions/workflows/ci.yml/badge.svg" alt="CI status" />
        <img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="Apache 2.0 license" />
        <Link to="/docs/security">Read the security notes →</Link>
      </div>
    </Section>
  );
}

/* ------------------------------------------------------------ timeline */

const SHIPPED = [
  {v: '2.2.3', d: '2026-10-15', t: 'MCP server and registry listing', p: 'paqtra-mcp ships as an OCI image with read-only tools and is listed as io.github.zyvorai/paqtra.'},
  {v: '2.2.3', d: '2026-10-15', t: 'Per-second metrics platform', p: 'Agent metrics with tiered storage, Hubble-derived series, anomaly detection and built-in alert rules.'},
  {v: 'main', d: 'unreleased', t: 'Envoy in front', p: 'Optional Gateway API route and backend TLS policy in the Helm chart.'},
];

export function Shipped(): ReactNode {
  return (
    <Section
      id="shipped"
      tint
      eyebrow="Shipped"
      title="What landed recently."
      lede="The full history is in the changelog.">
      <Reveal>
        <ul className={s.timeline}>
          {SHIPPED.map((r) => (
            <li key={r.t}>
              <span>{r.v} · {r.d}</span>
              <b>{r.t}</b>
              <p>{r.p}</p>
            </li>
          ))}
        </ul>
        <p><Link to="/docs/reference/changelog">Open the changelog →</Link></p>
      </Reveal>
    </Section>
  );
}

/* ----------------------------------------------------------- quickstart */

export function Quickstart(): ReactNode {
  return (
    <Section
      id="quickstart"
      eyebrow="Quickstart"
      title="Copy, paste, run."
      lede="You need a Kubernetes cluster, a kube-context and Cilium with Hubble Relay. Add --with-cilium to install Cilium when it is missing.">
      <Reveal>
        <div className={s.agents}>
          <Code>{`curl -fsSL https://raw.githubusercontent.com/zyvorai/zyvor-paqtra/main/install.sh | sh
# or: brew install zyvorai/tap/paqtra

paqtra install
paqtra status --wait
paqtra ui`}</Code>
          <div className={s.panelText}>
            <p>
              The installer verifies the sha256 sum, and the cosign signature when cosign is present. If something is wrong, <code>paqtra doctor</code> diagnoses it and <code>paqtra sysdump</code> collects a redacted support bundle.
            </p>
            <Link to="/docs/getting-started/quickstart">Full quickstart →</Link>
            {'  '}
            <Link to="/docs/getting-started/cli">CLI reference →</Link>
          </div>
        </div>
      </Reveal>
    </Section>
  );
}
