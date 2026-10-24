#!/usr/bin/env node
// Fills the npm packages for a release:
//   node npm/scripts/prepare.mjs <version> <dir with dist archives>
// Sets every package to <version>, pins the base package's optional
// dependencies to it, and extracts each platform's binary from the
// balzac-<target>.{tar.xz,zip} archive built by dist into its bin/.

import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const [version, archives] = process.argv.slice(2);
if (!version || !archives) {
  console.error("usage: prepare.mjs <version> <dir with dist archives>");
  process.exit(1);
}

const npmDir = resolve(import.meta.dirname, "..");
const repoDir = resolve(npmDir, "..");
// npm package (npm/platforms/<name>) -> Rust target built by dist
const TARGETS = JSON.parse(readFileSync(join(import.meta.dirname, "targets.json"), "utf8"));

const platformDirs = readdirSync(join(npmDir, "platforms")).sort();
const mapped = Object.keys(TARGETS).sort();
if (platformDirs.join() !== mapped.join()) {
  throw new Error(`npm/platforms (${platformDirs}) doesn't match npm/scripts/targets.json (${mapped})`);
}

function updatePackageJson(dir, update) {
  const path = join(dir, "package.json");
  const pkg = JSON.parse(readFileSync(path, "utf8"));
  update(pkg);
  writeFileSync(path, JSON.stringify(pkg, null, 2) + "\n");
  return pkg;
}

function extractBinary(target, binDir) {
  const windows = target.includes("windows");
  const exe = windows ? "balzac.exe" : "balzac";
  const archive = join(archives, `balzac-${target}${windows ? ".zip" : ".tar.xz"}`);
  if (!existsSync(archive)) {
    throw new Error(`missing dist archive ${archive}`);
  }
  const tmp = mkdtempSync(join(tmpdir(), "balzac-npm-"));
  try {
    if (windows) {
      execFileSync("unzip", ["-q", archive, exe, "-d", tmp]);
    } else {
      // dist's tarballs hold balzac-<target>/balzac
      execFileSync("tar", ["-xJf", archive, "-C", tmp, `balzac-${target}/${exe}`]);
    }
    const extracted = windows ? join(tmp, exe) : join(tmp, `balzac-${target}`, exe);
    rmSync(binDir, { recursive: true, force: true });
    mkdirSync(binDir);
    copyFileSync(extracted, join(binDir, exe));
    chmodSync(join(binDir, exe), 0o755);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

for (const [name, target] of Object.entries(TARGETS)) {
  const dir = join(npmDir, "platforms", name);
  updatePackageJson(dir, (pkg) => {
    pkg.version = version;
  });
  extractBinary(target, join(dir, "bin"));
  copyFileSync(join(repoDir, "LICENSE"), join(dir, "LICENSE"));
}

const baseDir = join(npmDir, "balzac");
updatePackageJson(baseDir, (pkg) => {
  pkg.version = version;
  pkg.optionalDependencies = Object.fromEntries(Object.keys(TARGETS).map((name) => [`@balzac/${name}`, version]));
});
copyFileSync(join(repoDir, "LICENSE"), join(baseDir, "LICENSE"));
copyFileSync(join(repoDir, "README.md"), join(baseDir, "README.md"));

console.log(`prepared balzac ${version} and ${platformDirs.length} platform packages`);
