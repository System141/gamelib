// Sets the app version everywhere it is written down: the workspace Cargo.toml (every crate
// and the Tauri app take it from there), Cargo.lock's entries for the workspace crates, and
// package.json.
//
//   node scripts/set-version.mjs 0.2.0     sets it
//   node scripts/set-version.mjs --print   prints the current one

import { readFileSync, writeFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url);
const CRATES = ["gamelib", "gamelib-cli", "gamelib-core"];

const read = (file) => readFileSync(new URL(file, ROOT), "utf8");
const write = (file, text) => writeFileSync(new URL(file, ROOT), text);

const cargo = read("Cargo.toml");
const WORKSPACE_VERSION = /(\[workspace\.package\][^[]*?\nversion = ")([^"]+)(")/;
const current = WORKSPACE_VERSION.exec(cargo)?.[2];
if (!current) fail("Cargo.toml has no [workspace.package] version");

const arg = process.argv[2];
if (arg === "--print") {
  console.log(current);
  process.exit(0);
}
if (!/^\d+\.\d+\.\d+$/.test(arg ?? "")) fail("usage: node scripts/set-version.mjs X.Y.Z");
const version = arg;

write("Cargo.toml", cargo.replace(WORKSPACE_VERSION, `$1${version}$3`));

let lock = read("Cargo.lock");
for (const name of CRATES) {
  const entry = new RegExp(`(\\[\\[package\\]\\]\\nname = "${name}"\\nversion = ")[^"]+(")`);
  if (!entry.test(lock)) fail(`${name} is not in Cargo.lock`);
  lock = lock.replace(entry, `$1${version}$2`);
}
write("Cargo.lock", lock);

const pkg = read("package.json");
if (!/"version": "[^"]+"/.test(pkg)) fail("package.json has no version");
write("package.json", pkg.replace(/"version": "[^"]+"/, `"version": "${version}"`));

console.log(`${current} -> ${version}`);

function fail(message) {
  console.error(message);
  process.exit(1);
}
