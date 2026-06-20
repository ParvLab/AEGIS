const fs = require("fs");
const path = require("path");

const src = path.resolve(__dirname, "..", "..", "..", "crates", "aegis-napi");
const dest = path.resolve(__dirname, "..", "node_modules", "@aegis-v", "engine");

if (fs.existsSync(dest)) {
  const stat = fs.lstatSync(dest);
  if (stat.isSymbolicLink() || stat.isDirectory()) {
    fs.rmSync(dest, { recursive: true, force: true });
  }
}

function copyDir(s, d) {
  fs.mkdirSync(d, { recursive: true });
  for (const e of fs.readdirSync(s)) {
    const sp = path.join(s, e);
    const dp = path.join(d, e);
    if (fs.statSync(sp).isDirectory() && e !== "target" && e !== "node_modules") {
      copyDir(sp, dp);
    } else if (!fs.statSync(sp).isDirectory()) {
      fs.copyFileSync(sp, dp);
    }
  }
}

copyDir(src, dest);
console.log("Copied @aegis-v/engine to node_modules/");
