#!/usr/bin/env node
/**
 * bump-version.mjs <version>
 *
 * Updates the app version in all 3 places (package.json, Cargo.toml,
 * tauri.conf.json) so a single tag push triggers a fresh release.
 *
 * Used by `release.bat`. Run directly:
 *     node scripts/bump-version.mjs 0.1.2
 */

import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
    console.error(`[bump] Invalid version: ${version}. Expected SEMVER like 0.1.2`);
    process.exit(1);
}

// 1. package.json
const pkgPath = resolve(ROOT, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
pkg.version = version;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
console.log(`[bump] package.json -> ${version}`);

// 2. src-tauri/Cargo.toml — replace the first `version = "..."` line.
const cargoPath = resolve(ROOT, "src-tauri", "Cargo.toml");
let cargo = readFileSync(cargoPath, "utf8");
cargo = cargo.replace(/^version\s*=\s*"[^"]*"/m, `version = "${version}"`);
writeFileSync(cargoPath, cargo);
console.log(`[bump] Cargo.toml -> ${version}`);

// 3. src-tauri/tauri.conf.json
const confPath = resolve(ROOT, "src-tauri", "tauri.conf.json");
const conf = JSON.parse(readFileSync(confPath, "utf8"));
conf.version = version;
writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n");
console.log(`[bump] tauri.conf.json -> ${version}`);

console.log(`[bump] Done.`);
