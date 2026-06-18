const fs = require('fs');
const path = require('path');

const targetDir = path.join(__dirname, '..', '..', '..', 'target', 'release');
const outDir = path.join(__dirname, '..');

const candidates = [
  { platform: 'win32', src: path.join(targetDir, 'aegis_napi.dll') },
  { platform: 'win32', src: path.join(targetDir, 'aegis_napi.pdb') },
  { platform: 'linux', src: path.join(targetDir, 'libaegis_napi.so') },
  { platform: 'darwin', src: path.join(targetDir, 'libaegis_napi.dylib') },
];

let found = false;
for (const c of candidates) {
  if (fs.existsSync(c.src)) {
    const dest = path.join(outDir, 'aegis-core.node');
    fs.copyFileSync(c.src, dest);
    if (process.platform === 'win32' && c.platform === 'win32') {
      const pdbSrc = path.join(targetDir, 'aegis_napi.pdb');
      if (fs.existsSync(pdbSrc)) {
        fs.copyFileSync(pdbSrc, path.join(outDir, 'aegis-core.pdb'));
      }
    }
    console.log(`copied ${c.platform} binary to ${dest}`);
    found = true;
    break;
  }
}

if (!found) {
  console.error('no native binary found in ' + targetDir);
  console.error('run `cargo build --package aegis-napi --release` first');
  process.exit(1);
}
