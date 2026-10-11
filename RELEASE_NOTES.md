# Release Notes

## Unreleased

### Changed
- README: installation from the release packages comes first (DMG, installer, deb/rpm/AppImage,
  bare CLI, checksums); project layout and source builds moved further down

## Version 0.1.0 (11.10.2026)

First public release.

### Added
- LLM Bridge: local OpenAI-compatible proxy for Anthropic Messages endpoints (LangDock, Anthropic),
  with streaming, token statistics, tray app (Tauri 2) and headless CLI
- Protection against browser requests (Origin/Host checks against CSRF and DNS rebinding),
  auto-generated local access token, no upstream redirects, private file permissions
- Signed automatic updates: the app installs a signed update once the proxy has been idle for
  five minutes; the CLI updates itself with `llm-bridge update [--check]`
- CI on GitHub (format, clippy, tests, MSRV 1.90, cargo-deny) and a release pipeline: macOS
  signed with a Developer ID and notarized, Windows and Linux unsigned with `SHA256SUMS`,
  update manifest `latest.json`
- Workspace lints and cargo-deny configuration

### Changed
- reqwest 0.13, toml 1, rusqlite 0.40, keyring 4
- Code formatted with rustfmt

### Fixed
- Release: notarization waits are retried after network drops on the build runners
