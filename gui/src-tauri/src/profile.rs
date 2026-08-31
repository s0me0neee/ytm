//! Where the frontend's own render timings land.
//!
//! React `DevTools` is the better tool for reading these — a flamegraph beats a
//! log file — but it needs someone at the keyboard looking at an Electron
//! window. This path needs nobody: the app writes what it measured, and the
//! file can be read afterwards, from a script, or by whoever is asking why a
//! release got slower. It is also the only one of the two that works in a
//! release build, where the `DevTools` hook is compiled out.
//!
//! Append-only JSON Lines, one object per batch, so a run can be `jq`-ed
//! without loading it whole and two runs can be diffed.

use std::io::Write;

use serde::Deserialize;

/// One React commit, as `<Profiler onRender>` reports it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    /// The `id` given to the `Profiler` that saw it.
    pub id: String,
    /// `"mount"` or `"update"`.
    pub phase: String,
    /// Milliseconds spent rendering this commit's subtree. The number that
    /// matters: it is CPU the webview spent, four times a second, for as long
    /// as a track is playing.
    pub actual_duration: f64,
    /// What the same subtree would cost with no memoisation at all. The gap
    /// between this and `actual_duration` is what `memo` is actually saving —
    /// if they are equal, every memo in that subtree is being defeated.
    pub base_duration: f64,
}

/// Appends a batch of commits to `render-profile.jsonl` beside `app.log`.
///
/// Errors are logged and swallowed. A profiler that can take the app down
/// with it is worse than no profiler, and a run that loses its last batch has
/// lost almost nothing.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command deserialises by value
pub fn log_render_timing(commits: Vec<Commit>) {
    if commits.is_empty() {
        return;
    }
    let path = ytm_core::session::config_toml_path().with_file_name("render-profile.jsonl");

    // Totals rather than every commit: a minute of playback is ~240 commits,
    // and what is being asked is "how much CPU per second", not "what did
    // commit 137 cost".
    let n = commits.len();
    let actual: f64 = commits.iter().map(|c| c.actual_duration).sum();
    let base: f64 = commits.iter().map(|c| c.base_duration).sum();
    let mut ids: Vec<&str> = commits.iter().map(|c| c.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    // Mounts are expected once; a stream of *updates* while nothing is being
    // interacted with is the thing worth noticing.
    let updates = commits.iter().filter(|c| c.phase == "update").count();

    let line = format!(
        r#"{{"at":{},"commits":{n},"updates":{updates},"actual_ms":{actual:.2},"base_ms":{base:.2},"ids":{:?}}}"#,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        ids,
    );

    let opened = std::fs::OpenOptions::new().create(true).append(true).open(&path);
    match opened {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{line}") {
                eprintln!("[gui] render profile write failed: {e}");
            }
        }
        Err(e) => eprintln!("[gui] render profile open failed: {e}"),
    }
}
