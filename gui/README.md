# ytm — desktop

The Tauri frontend for ytm: the same engine as the terminal app (`ytm-core`), with a window.

```bash
bun install
bun tauri dev     # a dev build, with hot reload of the frontend
bun tauri build   # a release bundle
```

`src/` is the React app (`views/` per page, `components/` shared); `src-tauri/` the Rust side.
The architecture notes live in the repository's top-level `CLAUDE.md`, under `gui/`.
