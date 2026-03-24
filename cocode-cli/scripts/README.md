# cocode-cli npm packaging

## Overview

`cocode-cli/` packages the `cocode` Rust binary (built from `cocode-rs/`) as an
npm-distributable CLI (`@cocode/cocode-cli`).

### Architecture

```
@cocode/cocode-cli                 (meta package, bin/cocode.js launcher)
  ├── @cocode/cocode-cli-linux-x64   (vendor/x86_64-unknown-linux-musl/cocode/cocode)
  ├── @cocode/cocode-cli-linux-arm64  (vendor/aarch64-unknown-linux-musl/cocode/cocode)
  ├── @cocode/cocode-cli-darwin-x64   (vendor/x86_64-apple-darwin/cocode/cocode)
  ├── @cocode/cocode-cli-darwin-arm64 (vendor/aarch64-apple-darwin/cocode/cocode)
  ├── @cocode/cocode-cli-win32-x64   (vendor/x86_64-pc-windows-msvc/cocode/cocode.exe)
  └── @cocode/cocode-cli-win32-arm64  (vendor/aarch64-pc-windows-msvc/cocode/cocode.exe)
```

The main package is lightweight (just `bin/cocode.js`). It declares the 6
platform packages as `optionalDependencies` — npm automatically installs only
the one matching the user's OS/arch.

## Prerequisites

- Python 3.10+
- Node.js 16+ / npm
- [GitHub CLI](https://cli.github.com/) (`gh`) — for downloading CI artifacts
- [zstd](https://github.com/facebook/zstd) — for extracting `.zst` archives
- Prebuilt `cocode` binaries from a CI workflow run

## Local development

### Populate vendor/ manually

If you have a locally built `cocode` binary, create the vendor tree by hand:

```bash
mkdir -p cocode-cli/vendor/x86_64-unknown-linux-musl/cocode
cp /path/to/cocode cocode-cli/vendor/x86_64-unknown-linux-musl/cocode/cocode
chmod +x cocode-cli/vendor/x86_64-unknown-linux-musl/cocode/cocode
```

Then test the launcher:

```bash
node cocode-cli/bin/cocode.js --help
```

### Populate vendor/ from CI artifacts

Once CI is configured, use the install script:

```bash
python3 cocode-cli/scripts/install_native_deps.py \
  --workflow-url https://github.com/<org>/<repo>/actions/runs/<run-id>
```

This downloads artifacts for all 6 targets and extracts them into
`cocode-cli/vendor/`.

## Building npm tarballs

### Option A: stage_npm_packages.py (recommended)

Stages the main package + all 6 platform packages in one command:

```bash
python3 cocode-cli/scripts/stage_npm_packages.py \
  --release-version 0.1.0 \
  --package cocode-cli
```

This:
1. Downloads native artifacts from the CI workflow for the release tag
2. Hydrates `vendor/` for each platform package
3. Writes tarballs to `dist/npm/`

Output:

```
dist/npm/
├── cocode-cli-npm-0.1.0.tgz              # main meta package
├── cocode-cli-npm-linux-x64-0.1.0.tgz
├── cocode-cli-npm-linux-arm64-0.1.0.tgz
├── cocode-cli-npm-darwin-x64-0.1.0.tgz
├── cocode-cli-npm-darwin-arm64-0.1.0.tgz
├── cocode-cli-npm-win32-x64-0.1.0.tgz
└── cocode-cli-npm-win32-arm64-0.1.0.tgz
```

To provide an explicit workflow URL instead of auto-resolving from the version
tag:

```bash
python3 cocode-cli/scripts/stage_npm_packages.py \
  --release-version 0.1.0 \
  --package cocode-cli \
  --workflow-url https://github.com/<org>/<repo>/actions/runs/<run-id>
```

### Option B: build_npm_package.py (individual packages)

For building a single package at a time. Requires `vendor/` to be pre-populated
via `install_native_deps.py`.

```bash
# Stage the main meta package (no native binaries needed)
python3 cocode-cli/scripts/build_npm_package.py \
  --package cocode-cli \
  --version 0.1.0 \
  --pack-output dist/cocode-cli.tgz

# Stage a platform package (needs --vendor-src)
python3 cocode-cli/scripts/build_npm_package.py \
  --package cocode-cli-linux-x64 \
  --version 0.1.0 \
  --vendor-src cocode-cli/vendor \
  --pack-output dist/cocode-cli-linux-x64.tgz
```

## Publishing to npm

### 1. Stage tarballs

```bash
python3 cocode-cli/scripts/stage_npm_packages.py \
  --release-version 0.1.0 \
  --package cocode-cli
```

### 2. Verify before publishing

```bash
# Inspect the main package
tar tzf dist/npm/cocode-cli-npm-0.1.0.tgz

# Inspect a platform package
tar tzf dist/npm/cocode-cli-npm-linux-x64-0.1.0.tgz
```

### 3. Publish platform packages first

Platform packages must be published before the main package, because the main
package declares them as `optionalDependencies`.

```bash
# Publish all platform packages
for tarball in dist/npm/cocode-cli-npm-linux-*.tgz \
               dist/npm/cocode-cli-npm-darwin-*.tgz \
               dist/npm/cocode-cli-npm-win32-*.tgz; do
  npm publish "$tarball" --access public
done
```

### 4. Publish the main package

```bash
npm publish dist/npm/cocode-cli-npm-0.1.0.tgz --access public
```

### 5. Verify installation

```bash
npm install -g @cocode/cocode-cli@0.1.0
cocode --version
```

## CI configuration (TODO)

To enable automated artifact download, set the constants at the top of:

- `scripts/install_native_deps.py` — `GITHUB_REPO` and `DEFAULT_WORKFLOW_URL`
- `scripts/stage_npm_packages.py` — `GITHUB_REPO` and `WORKFLOW_NAME`

The CI workflow should produce zstd-compressed artifacts named:

```
cocode-x86_64-unknown-linux-musl.zst
cocode-aarch64-unknown-linux-musl.zst
cocode-x86_64-apple-darwin.zst
cocode-aarch64-apple-darwin.zst
cocode-x86_64-pc-windows-msvc.exe.zst
cocode-aarch64-pc-windows-msvc.exe.zst
```

Each artifact should be uploaded under a directory named by its target triple.
