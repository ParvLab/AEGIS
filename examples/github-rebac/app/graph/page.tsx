"use client";

import { useEffect, useState, useRef, useCallback } from "react";
import dynamic from "next/dynamic";

const ForceGraph2D = dynamic(() => import("react-force-graph-2d"), {
  ssr: false,
  loading: () => (
    <div className="flex items-center justify-center h-96 bg-aegis-card rounded-xl border border-aegis-border">
      <p className="text-aegis-muted">Loading graph...</p>
    </div>
  ),
});

interface GraphNode {
  id: string;
  label: string;
  type: string;
}

interface GraphLink {
  source: string;
  target: string;
  relation: string;
}

interface GraphData {
  nodes: GraphNode[];
  links: GraphLink[];
}

const NODE_COLORS: Record<string, string> = {
  org: "#8b5cf6",
  team: "#3b82f6",
  repo: "#f59e0b",
  user: "#10b981",
};

const NODE_SIZES: Record<string, number> = {
  org: 18,
  team: 14,
  repo: 12,
  user: 10,
};

export default function GraphPage() {
  const containerRef = useRef<HTMLDivElement>(null);
  const [graphData, setGraphData] = useState<GraphData | null>(null);
  const [dimensions, setDimensions] = useState({ width: 600, height: 450 });
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [nodeTuples, setNodeTuples] = useState<Array<Record<string, unknown>>>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        setDimensions({ width: Math.floor(width), height: Math.max(400, Math.floor(height - 48)) });
      }
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  const fetchGraph = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const res = await fetch("/api/graph");
      const data = await res.json();
      if (data.error) { setError(data.error); } else { setGraphData(data); }
    } catch { setError("Failed to load graph data"); }
    setLoading(false);
  }, []);

  useEffect(() => { fetchGraph(); }, [fetchGraph]);

  async function handleNodeClick(node: GraphNode) {
    setSelectedNode(node);
    setNodeTuples([]);
    try {
      const isSubject = node.type === "user" || node.type === "team";
      const res = await fetch("/api/list", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          mode: isSubject ? "subject" : "object",
          target: node.id,
        }),
      });
      const data = await res.json();
      if (data.tuples) setNodeTuples(data.tuples);
    } catch { /* ignore */ }
  }

  const bannedNodes = new Set<string>();
  if (graphData) {
    for (const link of graphData.links) {
      if (link.relation === "banned") {
        bannedNodes.add(typeof link.source === "object" ? (link.source as GraphNode).id : link.source);
      }
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-aegis-text">Graph Explorer</h2>
        <p className="text-aegis-muted text-sm mt-1">
          Interactive access graph &mdash; click a node to see details, drag to rearrange
        </p>
      </div>

      <div className="grid grid-cols-1 xl:grid-cols-4 gap-6">
        <div ref={containerRef} className="xl:col-span-3 bg-aegis-card border border-aegis-border rounded-xl overflow-hidden" style={{ minHeight: 450 }}>
          {loading && (
            <div className="flex items-center justify-center h-96">
              <p className="text-aegis-muted">Loading graph data...</p>
            </div>
          )}
          {error && (
            <div className="flex items-center justify-center h-96">
              <p className="text-aegis-red">{error}</p>
            </div>
          )}
          {graphData && !loading && (
            <ForceGraph2D
              graphData={graphData}
              nodeLabel="label"
              nodeColor={(node: unknown) => {
                const n = node as GraphNode;
                if (bannedNodes.has(n.id)) return "#ef4444";
                return NODE_COLORS[n.type] || "#94a3b8";
              }}
              nodeVal={(node: unknown) => {
                const n = node as GraphNode;
                if (bannedNodes.has(n.id)) return (NODE_SIZES[n.type] || 8) * 1.4;
                return NODE_SIZES[n.type] || 8;
              }}
              linkColor={(link: unknown) => {
                const l = link as GraphLink;
                return l.relation === "banned" ? "#ef4444" : "#475569";
              }}
              linkWidth={(link: unknown) => {
                const l = link as GraphLink;
                return l.relation === "banned" ? 2.5 : 1;
              }}
              linkLabel={(link: unknown) => {
                const l = link as GraphLink;
                return l.relation === "banned" ? "⛔ banned (deny override)" : l.relation;
              }}
              linkDirectionalParticles={(link: unknown) => {
                const l = link as GraphLink;
                return l.relation === "banned" ? 4 : 0;
              }}
              linkDirectionalParticleSpeed={0.008}
              onNodeClick={(node: unknown) => handleNodeClick(node as GraphNode)}
              width={dimensions.width}
              height={dimensions.height}
              backgroundColor="#0f172a"
              d3AlphaDecay={0.02}
              d3VelocityDecay={0.3}
              enableNodeDrag={true}
              enableZoomInteraction={true}
              enablePanInteraction={true}
              nodeCanvasObject={(node: unknown, ctx: CanvasRenderingContext2D, globalScale: number) => {
                const n = node as GraphNode & { x: number; y: number };
                const label = n.label.split(":")[1] || n.label;
                const fontSize = Math.max(6, 12 / globalScale);
                const isBanned = bannedNodes.has(n.id);

                ctx.beginPath();
                ctx.arc(n.x, n.y, isBanned ? 7 : 5, 0, 2 * Math.PI, false);

                if (isBanned) {
                  ctx.strokeStyle = "#ef4444";
                  ctx.lineWidth = 3 / globalScale;
                  ctx.stroke();
                  ctx.fillStyle = "rgba(239, 68, 68, 0.3)";
                  ctx.fill();
                } else {
                  ctx.fillStyle = NODE_COLORS[n.type] || "#94a3b8";
                  ctx.fill();
                }

                ctx.font = `${fontSize}px Sans-Serif`;
                ctx.textAlign = "center";
                ctx.textBaseline = "middle";
                ctx.fillStyle = "#f1f5f9";
                ctx.fillText(label, n.x, n.y + 10 + 5 / globalScale);
              }}
            />
          )}
        </div>

        <div className="space-y-4">
          <div className="bg-aegis-card border border-aegis-border rounded-xl p-4">
            <p className="text-xs text-aegis-muted uppercase tracking-wider mb-3">Legend</p>
            <div className="space-y-2">
              <LegendItem color="#8b5cf6" label="Org" />
              <LegendItem color="#3b82f6" label="Team" />
              <LegendItem color="#f59e0b" label="Repo" />
              <LegendItem color="#10b981" label="User" />
              <div className="flex items-center gap-2">
                <div className="w-3 h-3 rounded-full bg-aegis-red" />
                <span className="text-xs text-aegis-text">Banned (red halo)</span>
              </div>
              <div className="flex items-center gap-2">
                <div className="w-4 h-0.5 bg-aegis-red" style={{ borderTop: "2px dashed #ef4444" }} />
                <span className="text-xs text-aegis-text">Deny override</span>
              </div>
            </div>
          </div>

          <button
            onClick={fetchGraph}
            className="w-full px-4 py-2 bg-aegis-card border border-aegis-border rounded-lg text-sm text-aegis-muted hover:text-aegis-text hover:border-aegis-accent/50 transition-colors"
          >
            🔄 Refresh Graph
          </button>

          {selectedNode && (
            <div className="bg-aegis-card border border-aegis-border rounded-xl p-4 animate-fade-in">
              <p className="text-xs text-aegis-muted uppercase tracking-wider mb-2">Selected</p>
              <p className="text-sm font-bold text-aegis-text font-mono">{selectedNode.label}</p>
              <p className="text-xs text-aegis-muted mt-1">Type: {selectedNode.type}</p>

              {nodeTuples.length > 0 && (
                <div className="mt-3 space-y-1">
                  {nodeTuples.slice(0, 8).map((t, i) => (
                    <p key={i} className="text-xs font-mono text-aegis-muted">
                      {String(t.relation)} → {String(t.object || t.subject)}
                    </p>
                  ))}
                  {nodeTuples.length > 8 && (
                    <p className="text-xs text-aegis-muted">...and {nodeTuples.length - 8} more</p>
                  )}
                </div>
              )}
            </div>
          )}

          {graphData && (
            <div className="bg-aegis-card border border-aegis-border rounded-xl p-4">
              <p className="text-xs text-aegis-muted uppercase tracking-wider mb-2">Stats</p>
              <p className="text-xs text-aegis-text">{graphData.nodes.length} nodes</p>
              <p className="text-xs text-aegis-text">{graphData.links.length} edges</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function LegendItem({ color, label }: { color: string; label: string }) {
  return (
    <div className="flex items-center gap-2">
      <div className="w-3 h-3 rounded-full" style={{ backgroundColor: color }} />
      <span className="text-xs text-aegis-text">{label}</span>
    </div>
  );
}
