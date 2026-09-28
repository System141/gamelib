// Writes the extra build configuration for signed update files (`tauri build --config <file>`):
// update bundles and signatures on, and the public key that verifies them built into the app.
// The key comes from the TAURI_UPDATER_PUBKEY variable, or from src-tauri/tauri.conf.json.
//
//   node scripts/updater-config.mjs <out.json>

import { readFileSync, writeFileSync } from "node:fs";

const out = process.argv[2];
if (!out) {
  console.error("usage: node scripts/updater-config.mjs <out.json>");
  process.exit(1);
}
const conf = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
const pubkey = (process.env.TAURI_UPDATER_PUBKEY || conf.plugins?.updater?.pubkey || "").trim();
if (!pubkey) {
  console.error(
    "A signing key is set but no public key: add the contents of the .key.pub file as the " +
      "TAURI_UPDATER_PUBKEY repository variable (or to plugins.updater.pubkey in src-tauri/tauri.conf.json).",
  );
  process.exit(1);
}
writeFileSync(out, JSON.stringify({ bundle: { createUpdaterArtifacts: true }, plugins: { updater: { pubkey } } }));
console.log("update files will be signed");
