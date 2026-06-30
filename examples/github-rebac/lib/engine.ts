import { initialize } from "@aegis-v/engine";
import { SCHEMA } from "./schema";
import path from "path";
import fs from "fs";

let _engine: any;
let _dbDir: string;
let _dbPath: string;

const globalForEngine = globalThis as unknown as {
  _engine: any;
};

function ensurePaths() {
  if (!_dbDir) {
    _dbDir = path.join(process.cwd(), ".aegis-data");
    _dbPath = path.join(_dbDir, "demo.db");
  }
}

export function readCurrentSchema(): string {
  ensurePaths();
  const schemaPath = path.join(_dbDir, "schema.yaml");
  if (fs.existsSync(schemaPath)) {
    return fs.readFileSync(schemaPath, "utf-8");
  }
  return SCHEMA;
}

export function writeCurrentSchema(yaml: string): void {
  ensurePaths();
  if (!fs.existsSync(_dbDir)) {
    fs.mkdirSync(_dbDir, { recursive: true });
  }
  const schemaPath = path.join(_dbDir, "schema.yaml");
  fs.writeFileSync(schemaPath, yaml, "utf-8");
}

export function getEngine(): any {
  if (process.env.NODE_ENV === "development") {
    if (globalForEngine._engine && typeof globalForEngine._engine.health === "function") {
      return globalForEngine._engine;
    }
  } else {
    if (_engine && typeof _engine.health === "function") {
      return _engine;
    }
  }

  ensurePaths();
  if (!fs.existsSync(_dbDir)) {
    fs.mkdirSync(_dbDir, { recursive: true });
  }
  const engineInstance = initialize(_dbPath, readCurrentSchema(), {
    maxReaders: 4,
    busyTimeoutMs: 5000,
    walMode: true,
  });

  if (process.env.NODE_ENV === "development") {
    globalForEngine._engine = engineInstance;
  } else {
    _engine = engineInstance;
  }
  return engineInstance;
}

export function resetEngine(): any {
  const engine = process.env.NODE_ENV === "development" ? globalForEngine._engine : _engine;
  if (engine && typeof engine.close === "function") {
    try {
      engine.close();
    } catch (e) {
      console.error("Failed to close engine connection:", e);
    }
  }

  ensurePaths();
  const fs = require("fs");
  for (const p of [_dbPath, _dbPath + "-wal", _dbPath + "-shm"]) {
    if (fs.existsSync(p)) {
      try {
        fs.unlinkSync(p);
      } catch (e: any) {
        console.warn(`Safe delete warning: Could not delete ${p} (${e.message})`);
      }
    }
  }

  if (process.env.NODE_ENV === "development") {
    globalForEngine._engine = null;
  } else {
    _engine = null;
  }
  return getEngine();
}
