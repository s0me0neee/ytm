import React, { Profiler, type ProfilerOnRenderCallback } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";

declare const __YTM_PROFILE__: boolean;

/** How often measured commits are handed to the backend. Batched rather than
 * sent per commit, because an `invoke` per render would be the thing doing
 * the most work in the profile. */
const FLUSH_MS = 2000;

/** Collects what React reports about each commit and posts it to
 * `log_render_timing`, which appends to `render-profile.jsonl`.
 *
 * The pair of durations is the interesting part. `actualDuration` is what the
 * commit cost; `baseDuration` is what it would have cost with no memoisation
 * at all. When the two converge, every `memo` in the tree is being defeated --
 * which is exactly what an unstable prop does, and exactly the bug that hid
 * here behind three inline arrow functions. */
function withProfiler(children: React.ReactNode) {
  const batch: unknown[] = [];

  const onRender: ProfilerOnRenderCallback = (id, phase, actualDuration, baseDuration) => {
    batch.push({ id, phase, actualDuration, baseDuration });
  };

  setInterval(() => {
    if (batch.length === 0) return;
    const commits = batch.splice(0, batch.length);
    invoke("log_render_timing", { commits }).catch(() => {});
  }, FLUSH_MS);

  return (
    <Profiler id="app" onRender={onRender}>
      {children}
    </Profiler>
  );
}

/* StrictMode double-invokes render in development, which roughly doubles
   every number React reports. That is fine for *comparing* two runs and
   wrong for quoting an absolute cost, so it is dropped while profiling --
   otherwise the file would record a figure the shipped app never pays. */
const tree = __YTM_PROFILE__ ? (
  withProfiler(<App />)
) : (
  <React.StrictMode>
    <App />
  </React.StrictMode>
);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(tree);
