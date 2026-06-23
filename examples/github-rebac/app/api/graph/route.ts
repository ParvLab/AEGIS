import { NextResponse } from "next/server";
import { getEngine } from "@/lib/engine";

export async function GET() {
  try {
    const engine = getEngine();
    
    // Retrieve all relationship tuples currently active in the database
    const queryResult = engine.query({}, { limit: 10000 });
    const parsed = typeof queryResult === "string" ? JSON.parse(queryResult) : queryResult;
    const tuples = parsed.tuples ?? [];

    const nodesMap = new Map<string, any>();
    const links: any[] = [];
    const seen = new Set<string>();

    for (const t of tuples) {
      const subj = t.subject;
      const obj = t.object;
      const rel = t.relation;

      const subjType = subj.split(":")[0];
      const objType = obj.split(":")[0];

      // Insert subject node
      if (!nodesMap.has(subj)) {
        nodesMap.set(subj, { id: subj, label: subj, type: subjType });
      }

      // Insert object node
      if (!nodesMap.has(obj)) {
        nodesMap.set(obj, { id: obj, label: obj, type: objType });
      }

      // Handle team members defined via subject sets (e.g. team:engineering#member)
      if (subjType === "team" && subj.includes("#")) {
        const baseTeam = subj.split("#")[0];
        if (!nodesMap.has(baseTeam)) {
          nodesMap.set(baseTeam, { id: baseTeam, label: baseTeam, type: "team" });
        }
        const key = `${baseTeam}|${rel}|${obj}`;
        if (!seen.has(key)) {
          seen.add(key);
          links.push({ source: baseTeam, target: obj, relation: rel });
        }
      } else {
        const key = `${subj}|${rel}|${obj}`;
        if (!seen.has(key)) {
          seen.add(key);
          links.push({ source: subj, target: obj, relation: rel });
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
