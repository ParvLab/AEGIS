import { initialize } from "@aegis-v/engine";
import { SCHEMA } from "./schema";
import path from "path";
import fs from "fs";

let _engine: any;
let _dbDir: string;
let _dbPath: string;

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
  if (_engine && typeof _engine.health === "function") {
    return _engine;
  }
  ensurePaths();
  if (!fs.existsSync(_dbDir)) {
    fs.mkdirSync(_dbDir, { recursive: true });
  }
  _engine = initialize(_dbPath, readCurrentSchema(), {
    maxReaders: 4,
    busyTimeoutMs: 5000,
    walMode: true,
  });
  return _engine;
}

export function resetEngine(): any {
  if (_engine && typeof _engine.close === "function") {
    _engine.close();
  }
  ensurePaths();
  const fs = require("fs");
  for (const p of [_dbPath, _dbPath + "-wal", _dbPath + "-shm"]) {
    if (fs.existsSync(p)) {
      fs.unlinkSync(p);
    }
  }
  _engine = null;
  return getEngine();
}
