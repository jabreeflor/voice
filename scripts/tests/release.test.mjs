// Tests for the push-to-main release: automatic versions, the update feed
// and its pre-publish check, and the pins between the files involved.
//
//   node --test scripts/tests/release.test.mjs
//
// The signature fixture under fixtures/release/ was produced by the real
// `cargo tauri signer sign` with a throwaway key (its private half was never
// committed), so the verifier is checked against what the bundler actually
// writes rather than against a format this file invented.

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { compare, nextVersion, parse, stampCargo, stampJson } from "../release-version.mjs";
import {
  archiveName,
  buildFeed,
  parsePublicKey,
  verifyFeed,
  verifySignature,
} from "../release-feed.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const FIX = path.join(ROOT, "scripts/tests/fixtures/release");
const read = (p) => fs.readFileSync(path.join(ROOT, p), "utf8");

const archive = fs.readFileSync(path.join(FIX, "Voice.app.tar.gz"));
const signature = fs.readFileSync(path.join(FIX, "Voice.app.tar.gz.sig"), "utf8");
const fixturePub = fs.readFileSync(path.join(FIX, "fixture.key.pub"), "utf8");

const conf = JSON.parse(read("crates/voice-app/tauri.conf.json"));
const release = JSON.parse(read("crates/voice-app/tauri.release.conf.json"));

// MARK: versions

test("parse accepts X.Y.Z only", () => {
  assert.deepEqual(parse("2.0.12"), [2, 0, 12]);
  assert.equal(parse("v2.0.0"), null);
  assert.equal(parse("2.0"), null);
  assert.equal(parse("2.0.0-beta.1"), null);
  assert.equal(parse("02.0.0"), null);
  assert.equal(compare([2, 10, 0], [2, 9, 99]), 1);
});

test("the first release ships the floor itself", () => {
  assert.equal(nextVersion("2.0.0", []), "2.0.0");
});

test("every later push is the next patch above the highest tag", () => {
  assert.equal(nextVersion("2.0.0", ["v2.0.0"]), "2.0.1");
  assert.equal(nextVersion("2.0.0", ["v2.0.0", "v2.0.9", "v2.0.10", "v2.0.2"]), "2.0.11");
});

test("raising the floor starts a new line", () => {
  assert.equal(nextVersion("2.1.0", ["v2.0.0", "v2.0.41"]), "2.1.0");
  assert.equal(nextVersion("2.1.0", ["v2.1.0"]), "2.1.1");
  assert.equal(nextVersion("3.0.0", ["v2.9.3"]), "3.0.0");
});

test("a floor below the tags never pulls the number backwards", () => {
  assert.equal(nextVersion("2.0.0", ["v2.3.4"]), "2.3.5");
});

test("tags that are not vX.Y.Z are ignored", () => {
  assert.equal(nextVersion("2.0.0", ["v2.0.3", "v9.9.9-rc.1", "nightly", "release-5", "vX"]), "2.0.4");
});

test("a malformed floor is refused, not guessed at", () => {
  assert.throws(() => nextVersion("2.0", []), /not X\.Y\.Z/);
});

test("stampJson rewrites only the top-level version", () => {
  const text = '{\n  "name": "x",\n  "version": "2.0.0",\n  "dep": {\n    "version": "9.9.9"\n  }\n}\n';
  const out = stampJson(text, "2.0.7");
  assert.equal(JSON.parse(out).version, "2.0.7");
  assert.equal(JSON.parse(out).dep.version, "9.9.9");
  assert.equal(out.length, text.length);
  assert.throws(() => stampJson('{"a": 1}', "2.0.7"), /no top-level/);
});

test("stampCargo rewrites only [workspace.package] version", () => {
  const text = [
    "[workspace.package]",
    'version = "2.0.0"',
    'edition = "2021"',
    "",
    "[workspace.dependencies]",
    'version = "9.9.9"',
    "",
  ].join("\n");
  const out = stampCargo(text, "2.0.7");
  assert.match(out, /^\[workspace\.package\]\nversion = "2\.0\.7"$/m);
  assert.match(out, /^\[workspace\.dependencies\]\nversion = "9\.9\.9"$/m);
  assert.throws(() => stampCargo("[package]\nversion = \"1.0.0\"\n", "2.0.7"), /workspace\.package/);
});

test("stamp works on the real tauri.conf.json and Cargo.toml", () => {
  const json = JSON.parse(stampJson(read("crates/voice-app/tauri.conf.json"), "7.8.9"));
  assert.equal(json.version, "7.8.9");
  assert.match(stampCargo(read("Cargo.toml"), "7.8.9"), /^version = "7\.8\.9"$/m);
});

test("the floor in tauri.conf.json and Cargo.toml agree", () => {
  const cargo = /\[workspace\.package\]\nversion = "([^"]+)"/.exec(read("Cargo.toml"));
  assert.ok(cargo, "Cargo.toml has no [workspace.package] version on the line after the header");
  assert.equal(cargo[1], conf.version);
});

// MARK: signatures

test("a real tauri signature verifies against its pubkey", () => {
  assert.match(verifySignature(archive, signature, fixturePub), /file:Voice\.app\.tar\.gz/);
});

test("a tampered archive is refused", () => {
  const tampered = Buffer.concat([archive, Buffer.from("x")]);
  assert.throws(() => verifySignature(tampered, signature, fixturePub), /does not verify/);
});

test("a signature from the wrong key names both key ids", () => {
  assert.throws(
    () => verifySignature(archive, signature, release.plugins.updater.pubkey),
    /signed by key [0-9A-F]{16}, but the app trusts key [0-9A-F]{16}/,
  );
});

test("a forged trusted comment is refused", () => {
  const text = Buffer.from(signature, "base64").toString("utf8");
  const forged = text.replace(/file:Voice\.app\.tar\.gz/, "file:evil.tar.gz");
  const b64 = Buffer.from(forged, "utf8").toString("base64");
  assert.throws(() => verifySignature(archive, b64, fixturePub), /trusted comment/);
});

test("the non-prehashed Ed mode verifies too", () => {
  const { publicKey, privateKey } = crypto.generateKeyPairSync("ed25519");
  const raw = Buffer.from(publicKey.export({ format: "jwk" }).x, "base64url");
  const keyId = crypto.randomBytes(8);
  const pub = Buffer.concat([Buffer.from("Ed"), keyId, raw]).toString("base64");
  const pubFile = Buffer.from(`untrusted comment: k\n${pub}\n`).toString("base64");
  const body = crypto.sign(null, archive, privateKey);
  const trusted = "timestamp:1";
  const global = crypto.sign(null, Buffer.concat([body, Buffer.from(trusted)]), privateKey);
  const sig = Buffer.concat([Buffer.from("Ed"), keyId, body]).toString("base64");
  const sigFile = Buffer.from(
    `untrusted comment: s\n${sig}\ntrusted comment: ${trusted}\n${global.toString("base64")}\n`,
  ).toString("base64");
  assert.equal(verifySignature(archive, sigFile, pubFile), trusted);
});

// MARK: the feed

const VERSION = "2.0.7";
const ARCHIVES = [archiveName(VERSION, "aarch64"), archiveName(VERSION, "x64")];
const sigs = (name) => (ARCHIVES.map((a) => `${a}.sig`).includes(name) ? `${signature}\n` : null);
const assets = (name) => (ARCHIVES.includes(name) ? archive : null);
const feed = () => buildFeed({ version: VERSION, repo: "jabreeflor/voice", readSig: sigs });

test("archives are named like the .dmg beside them", () => {
  assert.equal(archiveName("2.0.7", "aarch64"), "Voice_2.0.7_aarch64.app.tar.gz");
});

test("the feed lists both Mac architectures, each pointing at its own archive", () => {
  const f = feed();
  assert.equal(f.version, VERSION);
  assert.ok(!Number.isNaN(Date.parse(f.pub_date)));
  assert.deepEqual(Object.keys(f.platforms).sort(), ["darwin-aarch64", "darwin-x86_64"]);
  assert.equal(
    f.platforms["darwin-aarch64"].url,
    "https://github.com/jabreeflor/voice/releases/download/v2.0.7/Voice_2.0.7_aarch64.app.tar.gz",
  );
  assert.match(f.platforms["darwin-x86_64"].url, /Voice_2\.0\.7_x64\.app\.tar\.gz$/);
  // The .sig file's text, trimmed: the updater base64-decodes it whole.
  assert.equal(f.platforms["darwin-aarch64"].signature, signature.trim());
});

test("the feed is not written when an architecture did not build", () => {
  const onlyArm = (name) => (name === `${ARCHIVES[0]}.sig` ? signature : null);
  assert.throws(
    () => buildFeed({ version: VERSION, repo: "jabreeflor/voice", readSig: onlyArm }),
    /Voice_2\.0\.7_x64\.app\.tar\.gz\.sig/,
  );
});

test("a complete, correctly signed feed passes", () => {
  assert.deepEqual(verifyFeed(feed(), { version: VERSION, pubkey: fixturePub, read: assets }), []);
});

test("the feed is refused when its version is not the stamped one", () => {
  const errs = verifyFeed(feed(), { version: "2.0.8", pubkey: fixturePub, read: assets });
  assert.ok(errs.some((e) => /expected 2\.0\.8/.test(e)));
});

test("a missing architecture is refused", () => {
  const f = feed();
  delete f.platforms["darwin-x86_64"];
  assert.deepEqual(verifyFeed(f, { version: VERSION, pubkey: fixturePub, read: assets }), [
    "latest.json has no darwin-x86_64 entry — those installs would never update",
  ]);
});

test("an archive that was not built is refused", () => {
  const errs = verifyFeed(feed(), { version: VERSION, pubkey: fixturePub, read: () => null });
  assert.equal(errs.length, 2);
});

test("a feed signed by a key the app does not trust is refused", () => {
  const errs = verifyFeed(feed(), {
    version: VERSION,
    pubkey: release.plugins.updater.pubkey,
    read: assets,
  });
  assert.equal(errs.length, 2);
  assert.ok(errs.every((e) => /TAURI_SIGNING_PRIVATE_KEY is not the pair/.test(e)));
});

// MARK: pins between files

test("the shipped pubkey is a real minisign key", () => {
  assert.doesNotThrow(() => parsePublicKey(release.plugins.updater.pubkey));
});

test("only the release build knows the feed", () => {
  // updates.rs turns self-update on exactly when plugins.updater is in the
  // compiled config, so build.sh and `cargo tauri dev` builds never replace
  // themselves with whatever is on main.
  assert.equal(conf.plugins?.updater, undefined);
  assert.deepEqual(release.plugins.updater.endpoints, [
    "https://github.com/jabreeflor/voice/releases/latest/download/latest.json",
  ]);
  assert.equal(release.bundle.createUpdaterArtifacts, true);
});

test("release.yml ships on main and verifies the feed before publishing", () => {
  const wf = read(".github/workflows/release.yml");
  assert.match(wf, /push:\s*\n\s*branches:\s*\[main\]/);
  assert.match(wf, /release-version\.mjs next/);
  assert.match(wf, /release-version\.mjs stamp/);
  const publish = wf.slice(wf.search(/^ {2}publish:/m));
  assert.match(publish, /needs:\s*\[version, macos\]/);
  const write = publish.indexOf("release-feed.mjs write");
  const verify = publish.indexOf("release-feed.mjs verify");
  const create = publish.indexOf("gh release create");
  assert.ok(write > 0 && verify > write && create > verify, "publish must write, verify, then create");
});
