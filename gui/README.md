# ytm — desktop

The Tauri frontend for ytm: the same engine as the terminal app (`ytm-core`), with a window.

```bash
pnpm install
pnpm tauri dev     # a dev build, with hot reload of the frontend
pnpm tauri build   # a release bundle
```

`src/` is the React app (`views/` per page, `components/` shared); `src-tauri/` the Rust side.
The architecture notes live in the repository's top-level `CLAUDE.md`, under `gui/`.
