# Archived implementations

Rust/Tauri is the primary Queue Launcher implementation on `main` as of
November 19, 2026. All new features, fixes, and releases target
[Prototype/Tauri](../Prototype/Tauri).

These older implementations are retained for reference and are no longer
maintained or included in current releases.

| Implementation | Preserved source |
| --- | --- |
| Swift macOS app | [legacy/Sources](legacy/Sources), with its Swift package and packaging script in [legacy](legacy/README.md) |
| WinForms Windows app | [legacy/Windows](legacy/Windows), with tests and packaging script in [legacy](legacy/README.md) |
| Electron port | Git tag `archive/electron` at `31a5427`, originally `codex/electron-port` |
| Avalonia prototype | Git tag `archive/avalonia` at `c2e2098`, originally `codex/avalonia-prototype` |

The `archive/swift-winforms` tag at `dbec19d` also preserves the original
repository layout before the Rust/Tauri work. Existing development branches
and worktrees are retained. The current Rust browser harness remains with
the active app because it supports platform verification.

To inspect a tagged snapshot without changing your current checkout:

```sh
git fetch origin --tags
git show archive/electron:README.md
git show archive/avalonia:README.md
```
