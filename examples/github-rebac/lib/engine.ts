import { initialize } from "@aegis-v/engine";
import { SCHEMA } from "./schema";

let _engine: any;
let _dbDir: string;
let _dbPath: string;

function ensurePaths() {
  if (!_dbDir) {
    const path = require("path");
    _dbDir = path.join(process.cwd(), ".aegis-data");
    _dbPath = path.join(_dbDir, "demo.db");
  }
}

export function getEngine(): any {
  if (_engine && typeof _engine.health === "function") {
    return _engine;
  }
  ensurePaths();
  const fs = require("fs");
  if (!fs.existsSync(_dbDir)) {
    fs.mkdirSync(_dbDir, { recursive: true });
  }
  _engine = initialize(_dbPath, SCHEMA, {
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
