import { NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";
import { ALL_USERS, ALL_RESOURCES } from "@/lib/seed";

export async function GET() {
  try {
    const engine = getEngine();
    const nodesMap = new Map<string, any>();
    const links: any[] = [];
    const seen = new Set<string>();

    for (const obj of ALL_RESOURCES) {
      const tuples = engine.listByObject(obj);
      const type = obj.split(":")[0];
      if (!nodesMap.has(obj)) {
        nodesMap.set(obj, { id: obj, label: obj, type });
      }
      for (const t of tuples) {
        const subj = t.subject;
        const subjType = subj.split(":")[0];
        if (subjType === "team" && subj.includes("#")) {
          const baseTeam = subj.split("#")[0];
          if (!nodesMap.has(baseTeam)) {
            nodesMap.set(baseTeam, { id: baseTeam, label: baseTeam, type: "team" });
          }
          links.push({ source: baseTeam, target: obj, relation: t.relation });
        } else {
          if (!nodesMap.has(subj)) {
            nodesMap.set(subj, { id: subj, label: subj, type: subjType });
          }
          links.push({ source: subj, target: obj, relation: t.relation });
        }
      }
    }

    for (const subj of ALL_USERS) {
      const tuples = engine.listBySubject(subj);
      const subjType = subj.split(":")[0];
      if (!nodesMap.has(subj)) {
        nodesMap.set(subj, { id: subj, label: subj, type: subjType });
      }
      for (const t of tuples) {
        const key = `${subj}|${t.relation}|${t.object}`;
        if (!seen.has(key)) {
          seen.add(key);
          if (!nodesMap.has(t.object)) {
            const objType = t.object.split(":")[0];
            nodesMap.set(t.object, { id: t.object, label: t.object, type: objType });
          }
          links.push({ source: subj, target: t.object, relation: t.relation });
        }
      }
    }

    return NextResponse.json({
      nodes: Array.from(nodesMap.values()),
      links,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error", nodes: [], links: [] }, { status: 500 });
  }
}
