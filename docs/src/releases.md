# Releases

This page explains how changes move from a branch to a release: the branch rules, when a pre-release or a release is published, how the version number is chosen, what a release contains, how this documentation site is published, and the repository settings the workflows need once.

## Branches

- Work happens on `feature/<name>` and `bugfix/<name>` branches, merged into `develop` by pull request.
- `main` changes only through a pull request from `develop`. Pull requests into `main` must come from `develop` in this repository: CI's `source-branch` check fails any other, including one from a fork, and asks for it to target `develop` instead.
- Only merge commits are allowed. Squash and rebase merging are turned off, so `develop` and `main` never diverge.

## When something is published

The `release` workflow runs on every push to `develop` and `main`, normally a merged pull request:

| Push | What is published |
|---|---|
| To `develop`, touching the server | A **pre-release** `vX.Y.Z-dev.N`: a GitHub pre-release with the binaries, and the image tags `X.Y.Z-dev.N` and `develop` |
| To `main`, touching the server | A **release** `vX.Y.Z`: a GitHub release marked as the latest, with the binaries, and the image tags `X.Y.Z`, `X.Y`, `X` and `latest` |
| To `main`, anything | This documentation site, by the separate `docs` workflow |

- A push touches the server when it changes anything under `crates/`, or `Cargo.toml`, `Cargo.lock` or the `Dockerfile`, since the last release. On `develop` the last release is the nearest tag in the branch's history, pre-releases included; on `main` it is the newest stable tag. When nothing of that kind has changed since the last release, as after a change to documentation, scripts or workflows only, no release is published.
- If the tag for the computed version already exists, nothing is released.
- Each run tests the workspace before anything is drafted, then creates the release as a draft with its notes. The binaries and the image are built next, and the binaries are added to the draft. Only when both have succeeded is the draft published, which creates the git tag. If a build fails, the release stays a draft and no git tag is created.

Images are published to `ghcr.io/trozz/mcp-companies-house`. The `develop` tag follows the newest pre-release. In production, pin an exact version tag such as `ghcr.io/trozz/mcp-companies-house:1.2.3` rather than `latest`, and read the release notes before upgrading.

## Version numbers

`scripts/next-version.sh` computes the version. `scripts/test-next-version.sh` tests its rules.

- **Base.** The newest stable tag `vX.Y.Z`. Pre-release tags (`-dev.N`) and tags with other prefixes are ignored. With no stable tag yet, the base is `0.0.0`.
- **Bump.** It comes from the `develop` to `main` pull request: a `major`, `minor` or `patch` label, otherwise `[major]`, `[minor]` or `[patch]` in its title, otherwise `patch`. When there are several, the highest wins. A label beats the title, case does not matter, and other labels are ignored.
- **Release.** On `main`, the pull request is the one the merge commit came from, and the version is the base with that bump.
- **Pre-release.** On `develop`, the bump comes from the release pull request currently open from `develop` into `main`, or is `patch` when none is open. The version is the base with that bump, plus `-dev.N`, where `N` is the run number of the `release` workflow.

| Newest stable tag | Bump | Pre-release on `develop` | Release on `main` |
|---|---|---|---|
| none | `patch` | `v0.0.1-dev.N` | `v0.0.1` |
| `v1.4.2` | `patch` | `v1.4.3-dev.12` in run 12 | `v1.4.3` |
| `v1.2.7` | `minor` | `v1.3.0-dev.N` | `v1.3.0` |
| `v1.2.7` | `major` | `v2.0.0-dev.N` | `v2.0.0` |

A minor bump resets the patch number, and a major bump resets both the minor and patch numbers.

### Release preview

Every pull request into `main` shows a **Release preview** in the summary of CI's `release-preview` job: the bump, the version, the tag, whether merging it will publish a release, and the previous release. Adding or removing a label, or editing the title, runs CI again, so the preview follows the change.

## Release notes

Release notes are written by [git-cliff](https://git-cliff.org) from `cliff.toml`, using the conventional commit messages since the newest stable release. Pre-release tags are ignored, so a pre-release's notes also cover everything since the newest stable release.

| Commit type | Section |
|---|---|
| `feat` | Features |
| `fix` | Bug Fixes |
| `perf` | Performance |
| `refactor` | Refactor |
| `docs` | Documentation |

Commits of type `chore`, `ci`, `test`, `style` and `build`, merge commits and messages that do not follow the convention are left out. Each entry is the commit's description, with its scope when it has one, and the first seven characters of its hash. The workflow then adds install instructions for Docker, the binary and connecting a client.

## What a release contains

| Part | Details |
|---|---|
| Binaries | `mcp-companies-house-<version>-<target>.tar.gz` for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` and `aarch64-apple-darwin`. Each archive holds the binary, `README.md` and `LICENSE`, and comes with a `.sha256` file and a build provenance attestation |
| Container image | `ghcr.io/trozz/mcp-companies-house` for `linux/amd64` and `linux/arm64`. It is built `FROM scratch` with only the CA certificates and the binary, runs as UID 65532, and has an SBOM, provenance and a build provenance attestation pushed to the registry |
| Notes | As described under [Release notes](#release-notes), with install instructions |

The version is built into the binary, and `mcp-companies-house --version` prints it. See [Getting started](getting-started.md#with-the-binary) for verifying and running an archive.

## The documentation site

This site is published at [trozz.github.io/mcp-companies-house](https://trozz.github.io/mcp-companies-house/) by the `docs` workflow (`.github/workflows/docs.yml`).

- It runs on every push to `main`. It builds the site with `scripts/build-docs.sh`, which checks every internal link and anchor, and deploys it to GitHub Pages.
- It publishes whether or not the push makes a release, so a documentation-only change merged into `main` updates the site without a new version.
- It can also be started by hand from the Actions tab, but it publishes only from `main`.
- On pull requests, CI's `docs` job runs the same build and link check without publishing, so a broken link fails the pull request.
- The site therefore shows `main`. Changes merged into `develop` appear after the next merge of `develop` into `main`. Each page's edit link opens the file on `develop`, where changes go.

GitHub Pages must use **GitHub Actions** as its source; see [Repository setup](#repository-setup-once).

## Repository setup (once)

Maintainers do this once, when the repository is created:

```bash
git push origin main:develop
gh label create major --color B60205 --description "Release: major version bump"
gh label create minor --color FBCA04 --description "Release: minor version bump"
gh label create patch --color 0E8A16 --description "Release: patch version bump"
```

- Under **Settings > General > Pull Requests**, allow merge commits only: turn off squash merging and rebase merging.
- Under **Settings > Actions > General > Workflow permissions**, the workflows must be allowed to grant `contents: write`, `packages: write`, `id-token: write`, `attestations: write`, `pages: write` and `pull-requests: read`. The jobs request them explicitly through `permissions` blocks.
- Under **Settings > Pages**, set the source to **GitHub Actions**, or the `docs` workflow cannot publish the site.
- After the first image is published, set the `mcp-companies-house` package's visibility to public under **Packages** if you want anonymous pulls.
