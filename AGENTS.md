# Queue Launcher project instructions

- Rust/Tauri in `Tauri` is the maintained application. Older implementations
  in `archive` and archive tags are historical references.
- Keep Windows and macOS builds in parity. Every feature, UI change, and fix
  must apply to both platforms; update both native adapters when needed.
  Prefer shared Rust and UI code for behavior common to both platforms.
- Verify affected behavior on both platforms when their native environments
  are available. Clearly report platform checks that could not be run.
- Never use "Easiest fix:" as a justification for a decision or approach.
