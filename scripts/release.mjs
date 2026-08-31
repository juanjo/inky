#!/usr/bin/env node
/**
 * Assemble a GitHub-release folder in dist/release:
 *   - Inky_<version>_aarch64.dmg   (for new users)
 *   - Inky.app.tar.gz + .sig       (consumed by the auto-updater)
 *   - latest.json                  (the updater feed — upload as-is)
 *
 * The download URL is derived from the updater endpoint in tauri.conf.json;
 * override the repo with:  REPO=owner/name node scripts/release.mjs
 */
import fs from "node:fs";
import path from "node:path";

const root = path.join(import.meta.dirname, "..");
const conf = JSON.parse(fs.readFileSync(path.join(root, "src-tauri/tauri.conf.json"), "utf8"));
const version = conf.version;
const bundle = path.join(root, "src-tauri/target/release/bundle");

let repo = process.env.REPO;
if (!repo) {
  const endpoint = conf.plugins?.updater?.endpoints?.[0] ?? "";
  const m = endpoint.match(/github\.com\/([^/]+)\/([^/]+)\//);
  if (m) repo = `${m[1]}/${m[2]}`;
}
if (!repo || repo.includes("REPLACE_WITH")) {
  console.warn(
    "⚠️  Updater endpoint still has the placeholder repo. Set it in src-tauri/tauri.conf.json\n" +
      "   (or run with REPO=owner/name) — latest.json will contain a placeholder URL.",
  );
  repo ??= "REPLACE_WITH_YOUR_GITHUB_USER/inky";
}

const dmg = path.join(bundle, "dmg", `Inky_${version}_aarch64.dmg`);
const tarball = path.join(bundle, "macos", "Inky.app.tar.gz");
const sig = `${tarball}.sig`;
for (const f of [dmg, tarball, sig]) {
  if (!fs.existsSync(f)) {
    console.error(`Missing artifact: ${f}\nRun \`make dmg\` first.`);
    process.exit(1);
  }
}

const out = path.join(root, "dist", "release");
fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });
for (const f of [dmg, tarball, sig]) {
  fs.copyFileSync(f, path.join(out, path.basename(f)));
}

const latest = {
  version,
  pub_date: new Date().toISOString(),
  platforms: {
    "darwin-aarch64": {
      signature: fs.readFileSync(sig, "utf8").trim(),
      url: `https://github.com/${repo}/releases/download/v${version}/Inky.app.tar.gz`,
    },
  },
};
fs.writeFileSync(path.join(out, "latest.json"), JSON.stringify(latest, null, 2) + "\n");

console.log(`\nRelease v${version} assembled in dist/release:`);
for (const f of fs.readdirSync(out)) console.log(`  ${f}`);
console.log(
  `\nNext: create GitHub release "v${version}" on ${repo} and upload all four files.`,
);
