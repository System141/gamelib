// Writes the updater manifest (latest.json) for a release: every signed installer or bundle in
// <dir> (a `.sig` file next to it) under the platform keys the updater looks for. Linux has no
// plain `linux-x86_64` key on purpose: the updater falls back to it, and a .deb or .rpm install
// must never be handed an AppImage.
//
//   node scripts/latest-json.mjs <dir> <tag> <owner/repo> > latest.json

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const [dir, tag, repo] = process.argv.slice(2);
if (!dir || !/^v\d+\.\d+\.\d+$/.test(tag ?? "") || !/^[\w.-]+\/[\w.-]+$/.test(repo ?? "")) {
  console.error("usage: node scripts/latest-json.mjs <dir> vX.Y.Z <owner/repo>");
  process.exit(1);
}

const KEYS = [
  [(f) => f.endsWith("-setup.exe"), ["windows-x86_64", "windows-x86_64-nsis"]],
  [(f) => f.endsWith(".app.tar.gz"), ["darwin-aarch64", "darwin-x86_64", "darwin-aarch64-app", "darwin-x86_64-app"]],
  [(f) => f.endsWith(".AppImage"), ["linux-x86_64-appimage"]],
  [(f) => f.endsWith(".deb"), ["linux-x86_64-deb"]],
  [(f) => f.endsWith(".rpm"), ["linux-x86_64-rpm"]],
];

const platforms = {};
for (const file of readdirSync(dir).sort()) {
  const keys = KEYS.find(([matches]) => matches(file))?.[1];
  const sig = join(dir, `${file}.sig`);
  if (!keys || !existsSync(sig)) continue;
  const entry = {
    signature: readFileSync(sig, "utf8").trim(),
    url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(file)}`,
  };
  for (const key of keys) platforms[key] = entry;
}
if (Object.keys(platforms).length === 0) {
  console.error(`no signed installers in ${dir}`);
  process.exit(1);
}

const version = tag.slice(1);
console.log(JSON.stringify({ version, notes: `GameLib ${version}`, pub_date: new Date().toISOString(), platforms }, null, 2));
