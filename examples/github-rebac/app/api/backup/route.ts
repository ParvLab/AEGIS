import { NextRequest, NextResponse } from "next/server";
import { getEngine, resetEngine } from "@/lib/engine";
import path from "path";
import fs from "fs";

const BACKUPS_DIR = path.join(process.cwd(), ".aegis-data", "backups");

function ensureBackupsDir() {
  if (!fs.existsSync(BACKUPS_DIR)) {
    fs.mkdirSync(BACKUPS_DIR, { recursive: true });
  }
}

export async function GET(req: NextRequest) {
  try {
    ensureBackupsDir();
    const { searchParams } = new URL(req.url);
    const filename = searchParams.get("file");

    if (filename) {
      const safeFilename = path.basename(filename);
      const filePath = path.join(BACKUPS_DIR, safeFilename);
      if (!fs.existsSync(filePath)) {
        return NextResponse.json({ error: "Backup file not found" }, { status: 404 });
      }
      const fileBuffer = fs.readFileSync(filePath);
      return new NextResponse(fileBuffer, {
        headers: {
          "Content-Disposition": `attachment; filename="${safeFilename}"`,
          "Content-Type": "application/octet-stream",
        },
      });
    }

    const files = fs.readdirSync(BACKUPS_DIR);
    const list = files
      .filter(f => f.endsWith(".db"))
      .map(f => {
        const filePath = path.join(BACKUPS_DIR, f);
        const stat = fs.statSync(filePath);
        return {
          filename: f,
          sizeBytes: stat.size,
          createdAt: stat.birthtime.toISOString(),
        };
      });

    return NextResponse.json({ backups: list });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}

export async function POST(req: NextRequest) {
  try {
    ensureBackupsDir();
    const { action, filename, json } = await req.json();
    const engine = getEngine();

    if (action === "backup") {
      const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
      const name = `backup-${timestamp}.db`;
      const destPath = path.join(BACKUPS_DIR, name);

      // Trigger the Rust online WAL-safe backup API
      engine.backupToPath(destPath);

      return NextResponse.json({ success: true, filename: name });
    }

    if (action === "export-json") {
      const jsonText = engine.exportJson();
      return NextResponse.json({ json: jsonText });
    }

    if (action === "import-json") {
      if (!json) return NextResponse.json({ error: "Missing JSON to import" }, { status: 400 });
      const result = engine.importJson(json);
      return NextResponse.json({ success: true, revision: result.revision });
    }

    if (action === "restore") {
      if (!filename) return NextResponse.json({ error: "Missing backup filename to restore" }, { status: 400 });
      const safeFilename = path.basename(filename);
      const backupPath = path.join(BACKUPS_DIR, safeFilename);
      if (!fs.existsSync(backupPath)) {
        return NextResponse.json({ error: "Backup file not found" }, { status: 404 });
      }

      // Restore: close engine, copy backup file over current DB, clean wal/shm, reopen
      const dbPath = path.join(process.cwd(), ".aegis-data", "demo.db");
      engine.close();

      // Copy backup file
      fs.copyFileSync(backupPath, dbPath);

      // Remove wal/shm files if they exist
      for (const extra of [dbPath + "-wal", dbPath + "-shm"]) {
        if (fs.existsSync(extra)) {
          fs.unlinkSync(extra);
        }
      }

      // Re-initialize engine
      resetEngine();

      return NextResponse.json({ success: true });
    }

    return NextResponse.json({ error: `Unknown action: ${action}` }, { status: 400 });
  } catch (err: any) {
    return NextResponse.json({ error: err.message ?? "Unknown error" }, { status: 500 });
  }
}
