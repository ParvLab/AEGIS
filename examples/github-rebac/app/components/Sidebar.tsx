"use client";

import { useState } from "react";
import { usePathname } from "next/navigation";
import Link from "next/link";

const NAV_ITEMS = [
  { section: "Authorization", items: [
    { label: "Dashboard", href: "/", icon: "🔬" },
    { label: "Check", href: "/check", icon: "✓" },
    { label: "Check w/ Context", href: "/check-with-context", icon: "⚙️" },
    { label: "Explain", href: "/explain", icon: "🧬" },
    { label: "Who Can Access", href: "/who-can-access", icon: "🔍" },
    { label: "Simulation", href: "/simulate", icon: "🧪" },
  ]},
  { section: "Graph & Data", items: [
    { label: "Graph Explorer", href: "/graph", icon: "🕸️" },
    { label: "Entities", href: "/entities", icon: "👥" },
    { label: "Tuples", href: "/tuples", icon: "📝" },
    { label: "Audit", href: "/audit", icon: "📋" },
    { label: "Export", href: "/export", icon: "📤" },
  ]},
  { section: "Schema & Policies", items: [
    { label: "Schema Editor", href: "/schema", icon: "📐" },
    { label: "Policies", href: "/policies", icon: "📋" },
  ]},
  { section: "V7 Advanced", items: [
    { label: "Scheduler", href: "/scheduler", icon: "⏰" },
    { label: "Enforcement", href: "/enforcement", icon: "🛡️" },
    { label: "Events", href: "/events", icon: "📡" },
  ]},
  { section: "Engine Management", items: [
    { label: "Tenants / Partitions", href: "/partitions", icon: "🏢" },
    { label: "Analysis Suite", href: "/analysis", icon: "🔐" },
    { label: "Rate Limiter", href: "/rate-limiter", icon: "⏳" },
    { label: "Backup & Restore", href: "/backup", icon: "💾" },
    { label: "Actor Identity", href: "/actor", icon: "👤" },
    { label: "Error Playground", href: "/errors", icon: "⚠️" },
  ]},
];

export function Sidebar({ currentPath }: { currentPath: string }) {
  const [isOpen, setIsOpen] = useState(false);

  const isActive = (href: string) => {
    if (href === "/") return currentPath === "/";
    return currentPath.startsWith(href);
  };

  return (
    <>
      <button
        className="fixed top-4 left-4 z-50 md:hidden bg-aegis-card border border-aegis-border rounded-lg p-2 text-aegis-text"
        onClick={() => setIsOpen(!isOpen)}
        aria-label="Toggle sidebar"
      >
        <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
          <path d="M3 5h14M3 10h14M3 15h14" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
        </svg>
      </button>

      {isOpen && (
        <div className="fixed inset-0 z-40 bg-black/50 md:hidden" onClick={() => setIsOpen(false)} />
      )}

      <aside
        className={`
          fixed md:fixed inset-y-0 left-0 z-40 w-64
          bg-aegis-card border-r border-aegis-border
          transform transition-transform duration-200 ease-in-out
          ${isOpen ? "translate-x-0" : "-translate-x-full"}
          md:translate-x-0 overflow-y-auto
        `}
      >
        <div className="p-6 border-b border-aegis-border">
          <h1 className="text-lg font-bold text-aegis-accent">AEGIS</h1>
          <p className="text-xs text-aegis-muted mt-1 font-medium">ReBAC Engine Console</p>
        </div>

        <nav className="p-4 space-y-6">
          {NAV_ITEMS.map((group) => (
            <div key={group.section}>
              <p className="text-xs font-semibold text-aegis-muted uppercase tracking-wider mb-2 px-3">
                {group.section}
              </p>
              <div className="space-y-1">
                {group.items.map((item) => (
                  <Link
                    key={item.href}
                    href={item.href}
                    onClick={() => setIsOpen(false)}
                    className={`
                      flex items-center gap-3 px-3 py-2 rounded-lg text-sm transition-colors
                      ${isActive(item.href)
                        ? "bg-aegis-accent/10 text-aegis-accent font-semibold"
                        : "text-aegis-muted hover:text-aegis-text hover:bg-white/5"
                      }
                    `}
                  >
                    <span className="text-base">{item.icon}</span>
                    {item.label}
                  </Link>
                ))}
              </div>
            </div>
          ))}
        </nav>
      </aside>
    </>
  );
}

export function SidebarWrapper({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  return (
    <div className="flex min-h-screen bg-aegis-bg">
      <Sidebar currentPath={pathname} />
      <main className="flex-1 ml-0 md:ml-64 p-4 md:p-8 pt-16 md:pt-8 overflow-auto">
        <div className="max-w-6xl mx-auto animate-fade-in">
          {children}
        </div>
      </main>
    </div>
  );
}
