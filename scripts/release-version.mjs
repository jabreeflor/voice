#!/usr/bin/env node
// Automatic release versions for the push-to-main release
// (.github/workflows/release.yml).
//
//   node scripts/release-version.mjs next          # prints version=2.0.7 and tag=v2.0.7
//   node scripts/release-version.mjs stamp 2.0.7   # writes it into tauri.conf.json + Cargo.toml
//
// The version in crates/voice-app/tauri.conf.json is a *floor*, not the
// release number. Every push to main ships the next patch above the highest
// `v*` tag; to start a new minor or major, raise the floor (say to 2.1.0) in
// a PR and the next release is exactly that. Nothing is committed back to
// main: the stamp happens inside the CI checkout, and the tag is the record.
//
// The updater only installs a version that is semver-greater than the one
// running, so "strictly increasing, never reused" is the whole contract.
//
// Plain node with no dependencies, so it runs on any GitHub runner as is.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const CONF = path.join(ROOT, "crates/voice-app/tauri.conf.json");
export const CARGO = path.join(ROOT, "Cargo.toml");

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

/** "1.2.3" -> [1, 2, 3], or null for anything that is not plain X.Y.Z. */
export function parse(version) {
  const m = SEMVER.exec(String(version).trim());
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}

export function compare(a, b) {
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  return 0;
}

/**
 * The version the next release ships as.
 *
 * `floor` is tauri.conf.json's version; `tags` is every tag in the repo.
 * Tags that are not `vX.Y.Z` (pre-releases, other tooling) are ignored, so
 * they can neither block a release nor pull the number backwards.
 */
export function nextVersion(floor, tags) {
  const base = parse(floor);
  if (!base) throw new Error(`floor version ${JSON.stringify(floor)} is not X.Y.Z`);
  let highest = null;
  for (const tag of tags) {
    const t = String(tag).trim();
    if (!t.startsWith("v")) continue;
    const v = parse(t.slice(1));
    if (v && (!highest || compare(v, highest) > 0)) highest = v;
  }
  if (!highest || compare(base, highest) > 0) return base.join(".");
  return [highest[0], highest[1], highest[2] + 1].join(".");
}

/** Rewrite tauri.conf.json's top-level "version" without reformatting it. */
export function stampJson(text, version) {
  let replaced = 0;
  // Top-level only: two-space indent, as the file is formatted.
  const out = text.replace(/^( {2}"version":\s*)"[^"]*"/m, (_, head) => {
    replaced++;
    return `${head}"${version}"`;
  });
  if (replaced !== 1) throw new Error('no top-level "version" field to stamp');
  return out;
}

/**
 * Rewrite `version` under `[workspace.package]` in the root Cargo.toml, which
 * every crate inherits. Kept equal to tauri.conf.json so `cargo` and the
 * bundle agree on what was built.
 */
export function stampCargo(text, version) {
  const lines = text.split("\n");
  let section = "";
  let replaced = 0;
  for (let i = 0; i < lines.length; i++) {
    const header = /^\s*\[([^\]]+)\]\s*$/.exec(lines[i]);
    if (header) {
      section = header[1].trim();
      continue;
    }
    if (section === "workspace.package" && /^version\s*=/.test(lines[i])) {
      lines[i] = `version = "${version}"`;
      replaced++;
    }
  }
  if (replaced !== 1) throw new Error("no [workspace.package] version to stamp");
  return lines.join("\n");
}

function readFloor() {
  return JSON.parse(fs.readFileSync(CONF, "utf8")).version;
}

function gitTags() {
  const out = execFileSync("git", ["tag", "--list", "v*"], { cwd: ROOT, encoding: "utf8" });
  return out.split("\n").filter(Boolean);
}

function main(argv) {
  const [cmd, arg] = argv;
  if (cmd === "next") {
    const version = nextVersion(readFloor(), gitTags());
    process.stdout.write(`version=${version}\ntag=v${version}\n`);
    return 0;
  }
  if (cmd === "stamp") {
    if (!parse(arg ?? "")) {
      process.stderr.write(`stamp needs an X.Y.Z version (got ${JSON.stringify(arg)})\n`);
      return 2;
    }
    fs.writeFileSync(CONF, stampJson(fs.readFileSync(CONF, "utf8"), arg));
    fs.writeFileSync(CARGO, stampCargo(fs.readFileSync(CARGO, "utf8"), arg));
    process.stdout.write(`stamped ${arg} into tauri.conf.json and Cargo.toml\n`);
    return 0;
  }
  process.stderr.write("usage: release-version.mjs next | stamp <X.Y.Z>\n");
  return 2;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
