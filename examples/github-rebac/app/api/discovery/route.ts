import { NextResponse } from "next/server";
import { getEngine, readCurrentSchema } from "@/lib/engine";

function extractSchemaEntities(yaml: string) {
  const types: string[] = [];
  const relations = new Set<string>();
  const permissions = new Set<string>();

  const lines = yaml.split('\n');
  let currentType: string | null = null;
  let inRelations = false;
  let inPermissions = false;

  for (let line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;

    const indent = line.match(/^ */)?.[0].length ?? 0;

    if (indent === 2 && trimmed.includes(':')) {
      currentType = trimmed.split(':')[0].trim();
      types.push(currentType);
      inRelations = false;
      inPermissions = false;
    } else if (indent === 4 && (trimmed === 'relations:' || trimmed === 'relations: {}')) {
      inRelations = true;
      inPermissions = false;
    } else if (indent === 4 && (trimmed === 'permissions:' || trimmed === 'permissions: {}')) {
      inRelations = false;
      inPermissions = true;
    } else if (indent === 4 && trimmed.endsWith(':')) {
      inRelations = false;
      inPermissions = false;
    } else if (indent === 6 && trimmed.includes(':')) {
      const name = trimmed.split(':')[0].trim();
      if (inRelations) {
        relations.add(name);
      } else if (inPermissions) {
        permissions.add(name);
      }
    }
  }

  return {
    types,
    relations: Array.from(relations).sort(),
    permissions: Array.from(permissions).sort(),
  };
}

export async function GET() {
  try {
    const engine = getEngine();
    // Query all existing tuples up to 10000
    const queryResult = engine.query({}, { limit: 10000 });
    const parsed = typeof queryResult === "string" ? JSON.parse(queryResult) : queryResult;
    const tuples = parsed.tuples ?? [];

    const subjects = new Set<string>();
    const objects = new Set<string>();
    const relations = new Set<string>();
    const subjectTypes = new Set<string>();
    const objectTypes = new Set<string>();

    for (const t of tuples) {
      if (t.subject) {
        subjects.add(t.subject);
        subjectTypes.add(t.subject.split(":")[0]);
      }
      if (t.object) {
        objects.add(t.object);
        objectTypes.add(t.object.split(":")[0]);
      }
      if (t.relation) {
        relations.add(t.relation);
      }
    }

    const schemaText = readCurrentSchema();
    const schemaEntities = extractSchemaEntities(schemaText);

    // Merge entities parsed from schema to bootstrap when tuple store is empty
    for (const st of schemaEntities.types) {
      subjectTypes.add(st);
      objectTypes.add(st);
    }
    for (const rel of schemaEntities.relations) {
      relations.add(rel);
    }

    return NextResponse.json({
      subjects: Array.from(subjects).sort(),
      objects: Array.from(objects).sort(),
      relations: Array.from(relations).sort(),
      subjectTypes: Array.from(subjectTypes).sort(),
      objectTypes: Array.from(objectTypes).sort(),
      permissions: schemaEntities.permissions,
    });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
