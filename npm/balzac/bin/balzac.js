#!/usr/bin/env node
"use strict";

// npm installs only the @balzac/<platform> optional dependency matching this
// machine (package.json "os", "cpu" and "libc"); run the binary it ships.

const { spawn } = require("node:child_process");

function isMusl() {
  try {
    // glibc builds of node report the runtime glibc version, musl builds don't
    return !process.report.getReport().header.glibcVersionRuntime;
  } catch {
    return false;
  }
}

function candidatePackages() {
  const { platform, arch } = process;
  if (platform !== "linux") {
    return [`@balzac/${platform}-${arch}`];
  }
  // the musl binary is static, so it also runs on glibc systems; package
  // managers that ignore "libc" may have installed only that one
  return isMusl()
    ? [`@balzac/linux-${arch}-musl`]
    : [`@balzac/linux-${arch}-gnu`, `@balzac/linux-${arch}-musl`];
}

function binaryPath() {
  const exe = process.platform === "win32" ? "balzac.exe" : "balzac";
  const packages = candidatePackages();
  for (const pkg of packages) {
    try {
      return require.resolve(`${pkg}/bin/${exe}`);
    } catch {
      // not installed, try the next one
    }
  }
  console.error(
    `balzac: no binary found for ${process.platform}-${process.arch} (looked for ${packages.join(", ")}).\n` +
      "If your package manager skipped optional dependencies (e.g. --omit=optional or --no-optional), reinstall without that flag.\n" +
      "Otherwise this platform may not be supported: https://github.com/getbalzac/balzac/releases",
  );
  process.exit(1);
}

const child = spawn(binaryPath(), process.argv.slice(2), { stdio: "inherit" });

// Ctrl-C in a terminal reaches both processes, but a signal sent to this
// launcher alone (process managers, `kill`, CI timeouts) would otherwise leave
// the binary running without a parent
const forwarded = ["SIGINT", "SIGTERM", "SIGHUP"];
for (const signal of forwarded) {
  process.on(signal, () => child.kill(signal));
}

child.on("error", (error) => {
  console.error(`balzac: failed to run ${child.spawnfile}: ${error.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    // die from the same signal so callers see how the binary ended
    for (const s of forwarded) {
      process.removeAllListeners(s);
    }
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
