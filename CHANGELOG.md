# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Releases now build through `taiki-e/create-gh-release-action` and
  `taiki-e/upload-rust-binary-action` instead of hand-written packaging steps.
- Tests, `rustfmt` and `clippy` run in their own CI workflow on every push and
  pull request, rather than inside the release path.
- Every action is pinned to a commit SHA, with Dependabot keeping the pins
  current, and `actionlint` and `zizmor` audit the workflows on every push.

### Fixed

- `pf auth login` says that the key prompt is hidden, names the host that
  rejected a key, and `--help` states the default API root -- a key minted in
  one environment was previously indistinguishable from a mistyped one.

## [0.1.0] - 2026-09-05

First release.

### Added

- Commands covering all thirteen endpoints of the Passionfroot public API v1:
  placements, creators, labels, collaborations, conversations and their
  message timelines, inquiries, and proposal accept/reject.
- `pf creator message`, which resolves the single conversation a creator can
  have and either replies into it or opens it with an inquiry.
- `pf inbox` for unread, unarchived conversations with creator names attached.
- `pf api` as a direct escape hatch to any endpoint.
- `pf auth login` / `logout` / `status` / `token`, storing credentials in
  `~/.config/pf/config.toml` at mode 0600, with named profiles for multiple
  workspaces.
- Self-throttling to the documented 2 requests/second budget, shared across
  concurrent `pf` processes through a lock file.
- `--paginate` for cursor walking, `--dry-run` for previewing requests, and an
  `Idempotency-Key` on every write.
- Tables on a terminal, unmodified upstream JSON when piped, and stable exit
  codes for auth, not-found, conflict, rate-limit and network failures.

[Unreleased]: https://github.com/LaPulgaaa/pf-cli/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/LaPulgaaa/pf-cli/releases/tag/v0.1.0
