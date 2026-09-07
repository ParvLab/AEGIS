"use client";

const TYPE_DEFS = [
  {
    name: "Org",
    color: "border-aegis-purple/50",
    headerBg: "bg-aegis-purple/10",
    textColor: "text-aegis-purple",
    relations: ["member", "admin"],
    permissions: [
      { name: "view", includes: ["member", "admin"] },
      { name: "manage", includes: ["admin"] },
    ],
  },
  {
    name: "Team",
    color: "border-aegis-blue/50",
    headerBg: "bg-aegis-blue/10",
    textColor: "text-aegis-blue",
    relations: ["member", "maintainer", "admin"],
    permissions: [
      { name: "pull", includes: ["member", "maintainer", "admin"] },
      { name: "push", includes: ["maintainer", "admin"] },
      { name: "admin", includes: ["admin"] },
    ],
  },
  {
    name: "Repo",
    color: "border-aegis-amber/50",
    headerBg: "bg-aegis-amber/10",
    textColor: "text-aegis-amber",
    relations: ["viewer", "maintainer", "admin", "banned"],
    permissions: [
      { name: "pull", includes: ["viewer", "maintainer", "admin"] },
      { name: "push", includes: ["maintainer", "admin"] },
      { name: "admin", includes: ["admin"] },
    ],
    deny: ["banned"],
  },
];

export default function TypeCards() {
  return (
    <div>
      <p className="text-xs text-aegis-muted uppercase tracking-wider mb-3">Schema Types</p>
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        {TYPE_DEFS.map((type) => (
          <div key={type.name} className={`bg-aegis-card border ${type.color} rounded-xl overflow-hidden`}>
            <div className={`${type.headerBg} px-4 py-3 border-b ${type.color}`}>
              <p className={`text-sm font-bold ${type.textColor}`}>{type.name}</p>
            </div>
            <div className="p-4 space-y-3">
              <div>
                <p className="text-xs text-aegis-muted mb-1">Relations</p>
                <div className="flex flex-wrap gap-1">
                  {type.relations.map((r) => (
                    <span key={r} className={`text-xs px-2 py-0.5 rounded ${type.headerBg} ${type.textColor}`}>
                      {r}
                    </span>
                  ))}
                </div>
              </div>
              <div>
                <p className="text-xs text-aegis-muted mb-1">Permissions</p>
                <div className="space-y-1">
                  {type.permissions.map((p) => (
                    <div key={p.name} className="flex items-center gap-2 text-xs">
                      <span className={`font-mono font-medium ${type.textColor}`}>{p.name}</span>
                      <span className="text-aegis-muted">→</span>
                      <span className="text-aegis-muted">{p.includes.join(", ")}</span>
                    </div>
                  ))}
                </div>
              </div>
              {type.deny && (
                <div className="pt-2 border-t border-aegis-border">
                  <p className="text-xs text-aegis-red font-medium">⛔ Deny rules</p>
                  <p className="text-xs text-aegis-muted mt-1">
                    {type.deny.join(", ")} blocks all access
                  </p>
                </div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
