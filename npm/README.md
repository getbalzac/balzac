# npm packages

Balzac is published to npm the way esbuild and other native CLIs are (see
[Packaging Rust applications for the NPM registry](https://blog.orhun.dev/packaging-rust-for-npm/)):
no `postinstall` script and no download from GitHub at install time.

- [`balzac`](balzac) is a small launcher. Its `optionalDependencies` list one
  `@balzac/*` package per platform, and `bin/balzac.js` runs the binary of
  whichever one got installed, forwarding arguments, the exit code and
  signals.
- [`platforms/<name>`](platforms) hold the `@balzac/<name>` packages, each
  with a single binary in `bin/`. Their `os`, `cpu` and `libc` fields make
  package managers install only the one matching the machine.

| npm package                | Rust target                  |
| -------------------------- | ---------------------------- |
| `@balzac/darwin-arm64`     | `aarch64-apple-darwin`       |
| `@balzac/darwin-x64`       | `x86_64-apple-darwin`        |
| `@balzac/linux-arm64-gnu`  | `aarch64-unknown-linux-gnu`  |
| `@balzac/linux-arm64-musl` | `aarch64-unknown-linux-musl` |
| `@balzac/linux-x64-gnu`    | `x86_64-unknown-linux-gnu`   |
| `@balzac/linux-x64-musl`   | `x86_64-unknown-linux-musl`  |
| `@balzac/win32-arm64`      | `aarch64-pc-windows-msvc`    |
| `@balzac/win32-x64`        | `x86_64-pc-windows-msvc`     |

The musl binaries are static, so on glibc systems the launcher falls back to
them when the `-gnu` package is missing (package managers that ignore `libc`
may install only the musl one).

The committed `package.json` files stay at version `0.0.0`; versions, binaries,
`LICENSE` and the base package's `README.md` are filled in at release time by
[`scripts/prepare.mjs`](scripts/prepare.mjs) and are git-ignored.

## How a release flows

1. release-plz bumps the version, publishes the crate to crates.io and pushes
   the `vX.Y.Z` tag.
2. The tag triggers dist's [`release.yml`](../.github/workflows/release.yml),
   which builds the archives for every target, creates the GitHub Release
   and runs the publish jobs: the Homebrew formula and our
   [`publish-npm.yml`](../.github/workflows/publish-npm.yml).
3. `publish-npm.yml` downloads the archives built in that run, runs
   `scripts/prepare.mjs <version> target/distrib`, then
   [`scripts/publish.sh`](scripts/publish.sh), which publishes the platform
   packages first and `balzac` last, once npm has finished processing all of
   them (so it never points at missing packages). Versions already on npm,
   or still being processed, are skipped, so a failed job can simply be
   re-run. Prereleases go to the `next` dist-tag.

npm authenticates the job with
[trusted publishing](https://docs.npmjs.com/trusted-publishers): GitHub's
OIDC token is exchanged for a short-lived publish token, and provenance is
attached automatically. The publish job runs in the `npm` GitHub environment,
which the trusted publisher configuration requires. There is no npm token in the repository secrets.

## Changing targets

Targets are listed in three places that must agree:
`targets` in [`dist-workspace.toml`](../dist-workspace.toml) (then run
`dist generate`), [`scripts/targets.json`](scripts/targets.json) and the
`platforms/` directories, named `<process.platform>-<process.arch>` plus
`-gnu`/`-musl` on Linux, which is what `bin/balzac.js` looks for. Add the
matching `optionalDependencies` entry to
[`balzac/package.json`](balzac/package.json) too. Trusted publishing can only be
configured for a package that already exists, so publish a placeholder of a
new package from your machine (after `npm login`):

```sh
npx setup-trusted-publishing@1.0.2 --cwd npm/platforms/<name> --access public
```

then configure it as in [Trusted publishing setup](#trusted-publishing-setup)
shortly before the next release: npm expires a configuration that isn't used
within 2 days.

[`scripts/smoke-test.sh`](scripts/smoke-test.sh) checks all of this without
building balzac: it packs every package around a fake binary and runs the
launcher from a scratch install. CI runs it for changes under `npm/`.

## Trusted publishing setup

`balzac` and every `@balzac/*` package trust a GitHub Actions publisher:

- repository: `getbalzac/balzac`
- workflow filename: `release.yml` (not `publish-npm.yml`: npm checks the
  workflow that started the run, and `publish-npm.yml` is only called by it)
- environment: `npm`

Publishing access is set to "Require two-factor authentication and disallow
tokens". The `npm` GitHub environment only allows deployments from `v*`
tags.
