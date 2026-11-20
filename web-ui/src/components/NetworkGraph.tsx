// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useRef, useState } from 'react';
import { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation, type Simulation, type SimulationNodeDatum } from 'd3-force';
import { select } from 'd3-selection';
import { drag, type D3DragEvent } from 'd3-drag';

export type GraphNode = { id: string; label: string; group?: string };
export type GraphEdge = { source: string; target: string; label?: string };

type SimNode = GraphNode & SimulationNodeDatum;
type SimLink = { source: SimNode; target: SimNode; label?: string };

const GROUP_COLORS = [
  'var(--apple-blue)',
  'var(--accent-green)',
  'var(--accent-amber)',
  'var(--accent-purple)',
  'var(--accent-cyan)',
  'var(--danger)',
];

function groupColor(group: string | undefined, groups: string[]): string {
  if (!group) return GROUP_COLORS[0];
  const i = groups.indexOf(group);
  return GROUP_COLORS[i < 0 ? 0 : i % GROUP_COLORS.length];
}

type NetworkGraphProps = {
  nodes: GraphNode[];
  edges: GraphEdge[];
  label: string;
  height?: number;
};

/** Force-directed graph, driven by d3-force/d3-drag/d3-selection — no chart
 * library, matching DatapathHero's hand-rolled inline-SVG convention. */
export default function NetworkGraph({ nodes, edges, label, height = 420 }: NetworkGraphProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const simRef = useRef<Simulation<SimNode, SimLink> | null>(null);
  const [simNodes, setSimNodes] = useState<SimNode[]>([]);
  const W = 960;
  const H = height;

  const groups = useMemo(
    () => Array.from(new Set(nodes.map((n) => n.group).filter((g): g is string => Boolean(g)))),
    [nodes],
  );

  useEffect(() => {
    const initialNodes: SimNode[] = nodes.map((n) => ({ ...n }));
    const byId = new Map(initialNodes.map((n) => [n.id, n]));
    const simLinks: SimLink[] = edges.flatMap((e) => {
      const source = byId.get(e.source);
      const target = byId.get(e.target);
      return source && target ? [{ source, target, label: e.label }] : [];
    });

    const sim = forceSimulation(initialNodes)
      .force('link', forceLink<SimNode, SimLink>(simLinks).distance(90).strength(0.35))
      .force('charge', forceManyBody().strength(-220))
      .force('center', forceCenter(W / 2, H / 2))
      .force('collide', forceCollide(30))
      .on('tick', () => setSimNodes(sim.nodes().slice()));

    simRef.current = sim;
    setSimNodes(initialNodes);
    return () => {
      sim.stop();
      simRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodes, edges]);

  useEffect(() => {
    const sim = simRef.current;
    if (!svgRef.current || !sim) return;
    const behavior = drag<SVGGElement, SimNode>()
      .on('start', (event: D3DragEvent<SVGGElement, SimNode, SimNode>, d) => {
        if (!event.active) sim.alphaTarget(0.3).restart();
        d.fx = d.x;
        d.fy = d.y;
      })
      .on('drag', (event: D3DragEvent<SVGGElement, SimNode, SimNode>, d) => {
        d.fx = event.x;
        d.fy = event.y;
      })
      .on('end', (event: D3DragEvent<SVGGElement, SimNode, SimNode>, d) => {
        if (!event.active) sim.alphaTarget(0);
        d.fx = null;
        d.fy = null;
      });
    select(svgRef.current).selectAll<SVGGElement, SimNode>('g.ng-node').call(behavior);
  });

  const nodeById = new Map(simNodes.map((n) => [n.id, n]));
  const simEdges = edges.flatMap((e) => {
    const a = nodeById.get(e.source);
    const b = nodeById.get(e.target);
    return a && b ? [{ key: `${e.source}->${e.target}`, a, b }] : [];
  });

  if (nodes.length === 0) {
    return <p className="empty-state">No nodes to graph yet.</p>;
  }

  return (
    <figure className="ng-graph" aria-label={label}>
      <svg ref={svgRef} viewBox={`0 0 ${W} ${H}`} width="100%" height={H} role="img" aria-hidden="true">
        {simEdges.map((e) => (
          <line key={e.key} className="ng-link" x1={e.a.x} y1={e.a.y} x2={e.b.x} y2={e.b.y} />
        ))}
        {simNodes.map((n) => (
          <g key={n.id} className="ng-node" transform={`translate(${n.x ?? 0},${n.y ?? 0})`}>
            <circle r={10} style={{ fill: groupColor(n.group, groups) }} />
            <text className="ng-node-label" y={22} textAnchor="middle">
              {n.label}
            </text>
          </g>
        ))}
      </svg>
    </figure>
  );
}
