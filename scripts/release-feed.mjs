#!/usr/bin/env node
// The update feed (latest.json) for the push-to-main release.
//
//   node scripts/release-feed.mjs write  <dir> <version> <owner/repo>
//   node scripts/release-feed.mjs verify <dir> <version>
//
// <dir> holds the two updater archives the macOS jobs built and signed,
// `Voice_<version>_aarch64.app.tar.gz` and `Voice_<version>_x64.app.tar.gz`,
// each with its `.sig`. `write` turns them into latest.json; `verify` checks
// that file against the archives before anything is published.
//
// Publishing the release is what makes every installed copy fetch it
// (`releases/latest/download/latest.json`, tauri.release.conf.json), so
// `verify` is the last gate, and it answers the one question a green build
// cannot: will an installed Voice actually *accept* this update? That depends
// on three things the bundler never cross-checks:
//   * latest.json's version is the version this run stamped (a mismatch is a
//     feed installed copies either ignore or loop on);
//   * both darwin-aarch64 and darwin-x86_64 are present (the updater looks up
//     its own target and has no fallback to the other one);
//   * each signature verifies against the pubkey compiled into the app
//     (plugins.updater.pubkey in crates/voice-app/tauri.release.conf.json). A
//     TAURI_SIGNING_PRIVATE_KEY secret that is not the pair of that pubkey
//     signs every archive happily, and every installed copy then refuses every
//     update, silently, forever.
//
// The signature is minisign (what `tauri signer` writes), verified with
// node:crypto alone so this runs on a bare runner with no installs.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const RELEASE_CONF = path.join(ROOT, "crates/voice-app/tauri.release.conf.json");

/** Updater target -> the arch suffix tauri-bundler gives the macOS bundles. */
export const PLATFORMS = {
  "darwin-aarch64": "aarch64",
  "darwin-x86_64": "x64",
};

/** `Voice_2.0.7_aarch64.app.tar.gz`, named like the .dmg beside it. */
export function archiveName(version, arch) {
  return `Voice_${version}_${arch}.app.tar.gz`;
}

// MARK: minisign

/** Tauri stores both key and signature as base64 of the minisign text file. */
function minisignLines(b64, what) {
  const text = Buffer.from(String(b64).trim(), "base64").toString("utf8");
  const lines = text.split("\n").map((l) => l.replace(/\r$/, ""));
  if (!lines[0]?.startsWith("untrusted comment:")) {
    throw new Error(`${what} is not a base64 minisign file (no "untrusted comment:" line)`);
  }
  return lines;
}

export function parsePublicKey(pubkeyB64) {
  const lines = minisignLines(pubkeyB64, "pubkey");
  const raw = Buffer.from(lines[1] ?? "", "base64");
  if (raw.length !== 42 || raw.subarray(0, 2).toString("latin1") !== "Ed") {
    throw new Error("pubkey is not a minisign Ed25519 key");
  }
  return { keyId: raw.subarray(2, 10), key: raw.subarray(10, 42) };
}

function ed25519(rawKey) {
  return crypto.createPublicKey({
    key: { kty: "OKP", crv: "Ed25519", x: rawKey.toString("base64url") },
    format: "jwk",
  });
}

const hexId = (buf) => Buffer.from(buf).reverse().toString("hex").toUpperCase();

/**
 * Verify `data` against a tauri/minisign signature. Throws with a reason on
 * any mismatch; returns the trusted comment on success.
 */
export function verifySignature(data, signatureB64, pubkeyB64) {
  const pub = parsePublicKey(pubkeyB64);
  const lines = minisignLines(signatureB64, "signature");
  const sig = Buffer.from(lines[1] ?? "", "base64");
  if (sig.length !== 74) throw new Error("signature is not a minisign Ed25519 signature");
  const alg = sig.subarray(0, 2).toString("latin1");
  if (alg !== "Ed" && alg !== "ED") throw new Error(`unknown signature algorithm ${JSON.stringify(alg)}`);
  const keyId = sig.subarray(2, 10);
  if (!keyId.equals(pub.keyId)) {
    throw new Error(
      `signed by key ${hexId(keyId)}, but the app trusts key ${hexId(pub.keyId)} — ` +
        "TAURI_SIGNING_PRIVATE_KEY is not the pair of plugins.updater.pubkey",
    );
  }
  const key = ed25519(pub.key);
  // "ED" is minisign's prehashed mode: the signature covers BLAKE2b-512(data).
  const message = alg === "ED" ? crypto.createHash("blake2b512").update(data).digest() : data;
  if (!crypto.verify(null, message, key, sig.subarray(10))) {
    throw new Error("signature does not verify — the archive is not the one that was signed");
  }
  const trustedLine = lines[2] ?? "";
  if (!trustedLine.startsWith("trusted comment: ")) throw new Error("signature has no trusted comment");
  const trusted = trustedLine.slice("trusted comment: ".length);
  const globalSig = Buffer.from(lines[3] ?? "", "base64");
  const signedComment = Buffer.concat([sig.subarray(10), Buffer.from(trusted, "utf8")]);
  if (globalSig.length !== 64 || !crypto.verify(null, signedComment, key, globalSig)) {
    throw new Error("the trusted comment's signature does not verify");
  }
  return trusted;
}

// MARK: feed

/**
 * latest.json for `version`. `readSig(name)` returns the text of a `.sig`
 * file in the build directory, or null when it is missing.
 */
export function buildFeed({ version, repo, readSig, date = new Date() }) {
  const platforms = {};
  const missing = [];
  for (const [target, arch] of Object.entries(PLATFORMS)) {
    const file = archiveName(version, arch);
    const signature = readSig(`${file}.sig`);
    if (signature == null) {
      missing.push(`${file}.sig`);
      continue;
    }
    platforms[target] = {
      signature: signature.trim(),
      url: `https://github.com/${repo}/releases/download/v${version}/${file}`,
    };
  }
  if (missing.length) throw new Error(`missing ${missing.join(", ")} — did both macOS jobs upload?`);
  return {
    version,
    notes: `Voice ${version}`,
    pub_date: date.toISOString(),
    platforms,
  };
}

/**
 * Check a parsed latest.json against the expected version and the archives.
 * `read(name)` returns the bytes of a built archive or null. Returns every
 * problem found, so one run reports them all.
 */
export function verifyFeed(feed, { version, pubkey, read }) {
  const errors = [];
  const got = String(feed.version ?? "").replace(/^v/, "");
  if (got !== version) {
    errors.push(`latest.json says version ${JSON.stringify(feed.version)}, expected ${version}`);
  }
  const platforms = feed.platforms ?? {};
  for (const target of Object.keys(PLATFORMS)) {
    const entry = platforms[target];
    if (!entry) {
      errors.push(`latest.json has no ${target} entry — those installs would never update`);
      continue;
    }
    const url = String(entry.url ?? "");
    const file = decodeURIComponent(url.split("/").pop() ?? "");
    if (!url.includes(`/releases/download/v${version}/`)) {
      errors.push(`${target}: url ${entry.url} is not an asset of v${version}`);
    }
    const bytes = file ? read(file) : null;
    if (!bytes) {
      errors.push(`${target}: ${file || "(no url)"} is not among the built archives`);
      continue;
    }
    try {
      verifySignature(bytes, entry.signature ?? "", pubkey);
    } catch (e) {
      errors.push(`${target}: ${e.message}`);
    }
  }
  return errors;
}

export function releasePubkey() {
  return JSON.parse(fs.readFileSync(RELEASE_CONF, "utf8")).plugins?.updater?.pubkey ?? "";
}

function main(argv) {
  const [cmd, dir, version, repo] = argv;
  const usage = () => {
    process.stderr.write(
      "usage: release-feed.mjs write <dir> <X.Y.Z> <owner/repo> | verify <dir> <X.Y.Z>\n",
    );
    return 2;
  };
  if (!dir || !version) return usage();
  const feedPath = path.join(dir, "latest.json");
  if (cmd === "write") {
    if (!repo) return usage();
    const readSig = (name) => {
      const p = path.join(dir, name);
      return fs.existsSync(p) ? fs.readFileSync(p, "utf8") : null;
    };
    try {
      const feed = buildFeed({ version, repo, readSig });
      fs.writeFileSync(feedPath, `${JSON.stringify(feed, null, 2)}\n`);
    } catch (e) {
      process.stderr.write(`::error::${e.message}\n`);
      return 1;
    }
    process.stdout.write(`wrote ${feedPath}\n`);
    return 0;
  }
  if (cmd === "verify") {
    if (!fs.existsSync(feedPath)) {
      process.stderr.write(`::error::${feedPath} is missing\n`);
      return 1;
    }
    const feed = JSON.parse(fs.readFileSync(feedPath, "utf8"));
    const read = (name) => {
      const p = path.join(dir, name);
      return fs.existsSync(p) ? fs.readFileSync(p) : null;
    };
    const errors = verifyFeed(feed, { version, pubkey: releasePubkey(), read });
    if (errors.length) {
      for (const e of errors) process.stderr.write(`::error::${e}\n`);
      return 1;
    }
    process.stdout.write(
      `latest.json ${version}: ${Object.keys(PLATFORMS).join(", ")} signed by the key the app trusts\n`,
    );
    return 0;
  }
  return usage();
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
