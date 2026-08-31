//! Can every track in the library actually be shown at full quality, and how
//! long does it take?
//!
//! The question behind it is whether covers need caching at all. A cache
//! cannot make a picture *better* — it only stores whatever was fetched — so
//! it is worth having only if fetching is unreliable or slow. This measures
//! both against the real library rather than a handful of search hits.
//!
//! Three things are counted, and they are different questions.
//!
//! **Is a high-resolution copy obtainable?** Art-track covers arrive from
//! Google's image CDN with their size in the URL (`=w120-h120-l90-rj`), which
//! [`cover::at_size`] rewrites; the CDN serves any size up to 1400. A video's
//! thumbnail is a signed crop with no size to rewrite, so the only route up is
//! [`cover::hd_variant`]'s guess at `maxresdefault.jpg`, which exists only for
//! videos uploaded in HD. So the ceiling is not the same for the two kinds and
//! the report separates them.
//!
//! **What resolution comes back?** Not what was asked for, necessarily. The
//! decoded width is what the panel can actually draw, so that is what is
//! reported — against `NEEDED_PX`, the widest a cover is ever drawn in the
//! GUI's now-playing view on a 2x display.
//!
//! **How long does it take?** Per fetch, as a distribution rather than a mean:
//! the question is not the typical cover but the worst one, since that is the
//! one a user watches load.
//!
//! ```text
//! cargo run -p ytm-core --example cover_audit           # whole library
//! cargo run -p ytm-core --example cover_audit 40        # first 40 tracks
//! ```

/* A dev tool rather than shipped code: not built into either binary, run by
   hand against a live session. `clippy.toml` grants the same latitude to
   tests, which cargo has no equivalent of for examples -- so it is spelled
   out here instead. `large_futures` is the exception to that description:
   it ICEs the toolchain rather than reporting anything, on this crate's
   async fns. See the note in `ytm-core/src/lib.rs`. */
#![allow(clippy::large_futures)]
#![allow(clippy::arithmetic_side_effects)]
#![allow(clippy::as_conversions)]
#![allow(clippy::cast_precision_loss)]

use std::time::Instant;

use ytm_core::{Session, cover, library};

/// The widest a cover is drawn: `max-w-[28rem]` (448 CSS px) at 2x.
const NEEDED_PX: u32 = 896;

/// What the app asks the CDN for. Inside the 1400 the CDN will serve exactly.
const REQUEST_PX: u32 = 1200;

#[derive(Default)]
struct Tally {
    n: usize,
    ok: usize,
    /// Came back, but smaller than the panel can draw — the source's ceiling,
    /// not a failure of ours.
    short: usize,
    failed: usize,
    widths: Vec<u32>,
    millis: Vec<u128>,
}

impl Tally {
    fn report(&self, label: &str) {
        if self.n == 0 {
            return;
        }
        let pct = |k: usize| (k as f64) * 100.0 / (self.n as f64);
        println!("\n{label}  ({} tracks)", self.n);
        println!(
            "   full quality      {:>5}  {:5.1}%",
            self.ok,
            pct(self.ok)
        );
        println!(
            "   under {NEEDED_PX}px       {:>5}  {:5.1}%",
            self.short,
            pct(self.short)
        );
        println!(
            "   no image at all   {:>5}  {:5.1}%",
            self.failed,
            pct(self.failed)
        );

        let mut w = self.widths.clone();
        w.sort_unstable();
        if let (Some(min), Some(max)) = (w.first(), w.last()) {
            let mid = w.get(w.len() / 2).copied().unwrap_or(0);
            println!("   width  min {min}  median {mid}  max {max}");
        }

        let mut ms = self.millis.clone();
        ms.sort_unstable();
        if !ms.is_empty() {
            let at = |q: usize| ms.get(ms.len() * q / 100).copied().unwrap_or(0);
            println!(
                "   fetch  median {}ms  p90 {}ms  p99 {}ms  max {}ms",
                at(50),
                at(90),
                at(99),
                ms.last().copied().unwrap_or(0)
            );
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let limit: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    let session = Session::new()?;
    let yt = session.build_client()?;

    let playlists = library::get_playlists(&yt).await?;
    println!("{} playlists", playlists.len());

    // One flat list of (kind, url), so the two ceilings can be reported apart.
    let mut art = Tally::default();
    let mut video = Tally::default();
    let mut no_thumbnail = 0usize;
    let mut seen = 0usize;

    'outer: for pl in &playlists {
        let Some(tracks) = library::get_songs(&yt, &pl.playlist_id).await else {
            println!("   (couldn't load {:?} — skipping)", pl.title);
            continue;
        };
        println!("   {:<40} {} tracks", pl.title, tracks.len());

        for track in tracks {
            if seen >= limit {
                break 'outer;
            }
            let Some(raw) = track.thumbnail.as_deref() else {
                no_thumbnail += 1;
                continue;
            };
            seen += 1;

            // Exactly what the app asks for, by the same two rules.
            let sized = cover::at_size(raw, REQUEST_PX);
            let ladder = cover::hd_ladder(&sized);
            let tally = if ladder.is_empty() {
                &mut art
            } else {
                &mut video
            };
            tally.n += 1;

            let started = Instant::now();
            let mut got = None;
            for hd in &ladder {
                if let Ok(c) = cover::fetch(hd).await {
                    got = Some(Ok(c));
                    break;
                }
            }
            let got = match got {
                Some(got) => got,
                None => cover::fetch(&sized).await,
            };
            let elapsed = started.elapsed().as_millis();

            match got {
                Ok(c) => {
                    let w = c.width;
                    tally.millis.push(elapsed);
                    tally.widths.push(w);
                    if w >= NEEDED_PX {
                        tally.ok += 1;
                    } else {
                        tally.short += 1;
                        println!(
                            "      short: {w}px  {:?}",
                            track.title.as_deref().unwrap_or("?")
                        );
                    }
                }
                Err(e) => {
                    tally.failed += 1;
                    println!(
                        "      FAILED: {:?} — {e}",
                        track.title.as_deref().unwrap_or("?")
                    );
                }
            }
        }
    }

    art.report("art tracks (size-rewritable CDN URL)");
    video.report("videos (signed crop, maxresdefault guess)");
    if no_thumbnail > 0 {
        println!("\n{no_thumbnail} tracks carried no thumbnail URL at all");
    }

    let n = art.n + video.n;
    let ok = art.ok + video.ok;
    println!(
        "\noverall: {ok}/{n} at {NEEDED_PX}px or better ({:.1}%)",
        (ok as f64) * 100.0 / (n.max(1) as f64)
    );
    Ok(())
}
