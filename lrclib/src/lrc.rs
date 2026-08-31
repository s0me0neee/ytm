//! Parsing for the LRC synced-lyrics format returned in [`Lyrics::synced_lyrics`].
//!
//! A record looks like:
//!
//! ```text
//! [ar:Crusher-P]
//! [00:12.34]the quiet part before
//! [00:15.02]everything gets loud
//! [00:18.00]
//! [00:21.55][01:40.00]and I was still standing
//! ```
//!
//! Metadata tags are discarded, a line carrying several timestamps expands to
//! one entry per timestamp, and blank lines are kept — an empty [`LyricLine`]
//! marks an instrumental gap, which a renderer wants to show rather than
//! freezing on the previous lyric.
//!
//! [`Lyrics::synced_lyrics`]: crate::Lyrics::synced_lyrics

use std::time::Duration;

/// One timestamped lyric line.
#[derive(Debug, Clone, PartialEq)]
pub struct LyricLine {
    /// Offset from the start of the track, in seconds.
    pub at: f64,
    /// Line text, trimmed. Empty for interludes / instrumental gaps.
    pub text: String,
}

/// Parses LRC text into timestamped lines, sorted ascending by [`LyricLine::at`].
///
/// Metadata tags (`[ar:]`, `[ti:]`, `[al:]`, `[by:]`, `[length:]`, …) are
/// skipped, and `[offset:±ms]` shifts every timestamp. Lines carrying no
/// timestamp at all are dropped. Malformed input yields fewer lines rather than
/// an error — this never panics.
#[must_use]
pub fn parse_lrc(src: &str) -> Vec<LyricLine> {
    let mut out: Vec<LyricLine> = Vec::new();
    let mut offset_secs = 0.0f64;

    for raw in src.lines() {
        let line = raw.trim_end_matches('\r');
        let mut rest = line.trim_start();
        let mut stamps: Vec<f64> = Vec::new();

        // Peel leading `[...]` groups. Each is either a timestamp or a tag.
        while let Some(inner) = rest.strip_prefix('[') {
            // Unclosed bracket — treat the remainder as text.
            let Some((content, after)) = inner.split_once(']') else {
                break;
            };

            if let Some(secs) = parse_timestamp(content) {
                stamps.push(secs);
            } else if let Some(v) = content.strip_prefix("offset:") {
                // Positive offset shifts lyrics earlier, per the de-facto convention.
                //
                // `is_finite` is not defensive dressing. `f64::from_str` accepts
                // "nan", "inf" and "infinity", and lrclib's records are
                // user-submitted -- so `[offset:nan]` in a header is enough to
                // make every `at` below NaN, which `total_cmp` will happily
                // sort and every consumer downstream then has to survive.
                if let Ok(ms) = v.trim().trim_start_matches('+').parse::<f64>()
                    && ms.is_finite()
                {
                    offset_secs = ms / 1000.0;
                }
            }
            // Any other tag is metadata — discarded.

            rest = after;
        }

        // No timestamp means metadata-only or garbage; drop the whole line.
        if stamps.is_empty() {
            continue;
        }

        let text = rest.trim();
        for at in stamps {
            out.push(LyricLine {
                at: at - offset_secs,
                text: text.to_string(),
            });
        }
    }

    // Stable, so multi-timestamp lines sharing an `at` keep their source order.
    out.sort_by(|a, b| a.at.total_cmp(&b.at));
    out
}

/// Parses `mm:ss`, `mm:ss.xx`, `mm:ss.xxx` or `hh:mm:ss.xx` into seconds.
///
/// Every component must be ASCII digits, which is what makes tags like
/// `ar:Crusher-P` and `length:03:57` reject rather than parse as a time.
fn parse_timestamp(content: &str) -> Option<f64> {
    // `mm:ss` or `hh:mm:ss`, and nothing else. Taken apart by pattern rather
    // than by index: the shapes are the two the format has, so saying them
    // directly leaves no position to be out of range and no length to
    // subtract from.
    let mut it = content.split(':');
    let (first, second, third) = (it.next()?, it.next()?, it.next());
    if it.next().is_some() {
        return None;
    }
    let (hours_str, mins_str, last) = third.map_or(("0", first, second), |t| (first, second, t));

    // The last component carries the optional fractional part.
    let (secs_str, frac) = match last.split_once('.') {
        Some((s, f)) => (s, Some(f)),
        None => (last, None),
    };

    let is_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());

    if !is_digits(hours_str) || !is_digits(mins_str) || !is_digits(secs_str) {
        return None;
    }

    // "0.{frac}" handles .x / .xx / .xxx uniformly.
    let frac_secs = match frac {
        Some(f) if is_digits(f) => format!("0.{f}").parse::<f64>().ok()?,
        Some(_) => return None,
        None => 0.0,
    };

    let secs: f64 = secs_str.parse().ok()?;
    let mins: f64 = mins_str.parse().ok()?;
    let hours: f64 = hours_str.parse().ok()?;

    // Every component is ASCII digits, but `f64::from_str` saturates to
    // infinity rather than erroring, so a long enough run of them parses
    // rather than rejects. Checked once at the end, which covers the
    // components and their sum together.
    let total = hours.mul_add(3600.0, mins * 60.0) + secs + frac_secs;
    total.is_finite().then_some(total)
}

/// Index of the line active at `t` seconds, or `None` before the first
/// timestamp (i.e. during an intro). After the last timestamp the final line
/// stays active.
///
/// `lines` must be sorted as [`parse_lrc`] returns it.
#[must_use]
pub fn active_index(lines: &[LyricLine], t: f64) -> Option<usize> {
    if t < lines.first()?.at {
        return None;
    }
    // Sorted input makes the predicate true-then-false, so this is a valid
    // O(log n) binary search.
    //
    // `checked_sub` rather than `- 1`, even though `lines[0].at <= t` ought to
    // guarantee a partition point of at least 1. It does not when a timestamp
    // is NaN: `t < NaN` is false, so the guard above lets it through, and then
    // `NaN <= t` is false too, so the partition point is 0 and the subtraction
    // wraps. `parse_lrc` no longer produces NaN, but this is a `pub` function
    // over a caller's slice and the honest answer to "no line is at or before
    // t" is `None` rather than a panic.
    let i = lines.partition_point(|l| l.at <= t);
    i.checked_sub(1)
}

/// Seconds until the next line boundary strictly after `t`, or `None` once past
/// the last line. Used to schedule a redraw exactly when the highlight moves.
///
/// Always representable as a [`Duration`] when `Some`, which is the caller's
/// only use for it. Neither NaN nor a gap of 10^45 seconds can be a sleep, and
/// `Duration::from_secs_f64` panics on both -- so the test is representability
/// rather than mere finiteness, and the answer for anything else is "no
/// boundary", leaving the caller on its idle interval.
#[must_use]
pub fn next_boundary(lines: &[LyricLine], t: f64) -> Option<f64> {
    let i = lines.partition_point(|l| l.at <= t);
    lines
        .get(i)
        .map(|l| l.at - t)
        .filter(|&dt| Duration::try_from_secs_f64(dt).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(lines: &[LyricLine], i: usize) -> (f64, &str) {
        (lines[i].at, lines[i].text.as_str())
    }

    #[test]
    fn parses_two_digit_fraction() {
        let l = parse_lrc("[00:12.34]hello");
        assert_eq!(l.len(), 1);
        assert!((l[0].at - 12.34).abs() < 1e-9);
        assert_eq!(l[0].text, "hello");
    }

    #[test]
    fn parses_three_digit_fraction() {
        let l = parse_lrc("[01:02.500]x");
        assert!((l[0].at - 62.5).abs() < 1e-9);
    }

    #[test]
    fn parses_bare_mm_ss() {
        let l = parse_lrc("[02:03]x");
        assert!((l[0].at - 123.0).abs() < 1e-9);
    }

    #[test]
    fn parses_hh_mm_ss() {
        let l = parse_lrc("[01:00:05.50]x");
        assert!((l[0].at - 3605.5).abs() < 1e-9);
    }

    #[test]
    fn multiple_timestamps_expand_to_multiple_lines() {
        let l = parse_lrc("[00:12.00][01:40.00]chorus");
        assert_eq!(l.len(), 2);
        assert_eq!(at(&l, 0), (12.0, "chorus"));
        assert_eq!(at(&l, 1), (100.0, "chorus"));
    }

    #[test]
    fn metadata_tags_are_skipped() {
        let src =
            "[ar:Crusher-P]\n[ti:Echo]\n[al:Album]\n[by:someone]\n[length:03:57]\n[00:10.00]real";
        let l = parse_lrc(src);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].text, "real");
    }

    #[test]
    fn length_tag_does_not_parse_as_timestamp() {
        // `length` is not digits, so the tag must not become a 3:57 line.
        assert!(parse_timestamp("length:03:57").is_none());
        assert!(parse_timestamp("ar:Foo").is_none());
    }

    #[test]
    fn offset_shifts_timestamps_earlier() {
        let l = parse_lrc("[offset:+500]\n[00:10.00]x");
        assert!((l[0].at - 9.5).abs() < 1e-9);
    }

    #[test]
    fn negative_offset_shifts_later() {
        let l = parse_lrc("[offset:-500]\n[00:10.00]x");
        assert!((l[0].at - 10.5).abs() < 1e-9);
    }

    #[test]
    fn out_of_order_input_is_sorted() {
        let l = parse_lrc("[00:30.00]c\n[00:10.00]a\n[00:20.00]b");
        assert_eq!(
            l.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn blank_interlude_is_preserved() {
        let l = parse_lrc("[00:10.00]a\n[00:20.00]\n[00:30.00]b");
        assert_eq!(l.len(), 3);
        assert_eq!(l[1].text, "");
    }

    #[test]
    fn handles_crlf() {
        let l = parse_lrc("[00:10.00]a\r\n[00:20.00]b\r\n");
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].text, "a");
        assert_eq!(l[1].text, "b");
    }

    #[test]
    fn line_without_brackets_is_ignored() {
        let l = parse_lrc("just some prose\n[00:10.00]a");
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].text, "a");
    }

    #[test]
    fn unclosed_bracket_does_not_panic() {
        assert_eq!(parse_lrc("[00:10.00"), []);
        assert_eq!(parse_lrc("["), []);
        let l = parse_lrc("[00:10.00]ok\n[unclosed");
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn empty_input_yields_nothing() {
        assert_eq!(parse_lrc(""), []);
        assert_eq!(parse_lrc("\n\n\n"), []);
    }

    #[test]
    fn active_index_covers_all_positions() {
        let l = parse_lrc("[00:10.00]a\n[00:20.00]b\n[00:30.00]c");

        assert_eq!(active_index(&l, 0.0), None, "before the first stamp");
        assert_eq!(active_index(&l, 9.99), None);
        assert_eq!(active_index(&l, 10.0), Some(0), "exactly on a boundary");
        assert_eq!(active_index(&l, 15.0), Some(0), "between boundaries");
        assert_eq!(active_index(&l, 20.0), Some(1));
        assert_eq!(
            active_index(&l, 999.0),
            Some(2),
            "past the last stays on it"
        );
    }

    #[test]
    fn active_index_on_empty_is_none() {
        assert_eq!(active_index(&[], 5.0), None);
    }

    /* The parser's promise is that it never panics, and for a while it did
       not keep it: `parse_lrc` accepted values that made its *consumers*
       panic rather than panicking itself. Every case below was reachable
       from a record on lrclib.net, whose lyrics are user-submitted. */

    #[test]
    fn a_nan_offset_is_ignored_rather_than_poisoning_every_timestamp() {
        // `f64::from_str` accepts "nan"; unfiltered it made every `at` NaN.
        let l = parse_lrc("[offset:nan]\n[00:01.00]one\n[00:05.00]two");
        assert_eq!(l.len(), 2);
        assert!(l.iter().all(|x| x.at.is_finite()), "{l:?}");
        assert!((l[0].at - 1.0).abs() < 1e-9, "the offset was dropped, not applied");
    }

    #[test]
    fn an_infinite_offset_is_ignored_too() {
        let l = parse_lrc("[offset:1e400]\n[00:01.00]one");
        assert!((l[0].at - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_timestamp_too_big_for_f64_is_rejected() {
        // Every component is ASCII digits, so the only thing that can reject
        // this is the finiteness check on the sum.
        let huge = "9".repeat(320);
        let l = parse_lrc(&format!("[00:01.00]kept\n[{huge}:00.00]dropped"));
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].text, "kept");
    }

    #[test]
    fn active_index_answers_none_rather_than_underflowing() {
        // A NaN timestamp defeats both guards in `active_index`: `t < NaN` is
        // false so it is not an early return, and `NaN <= t` is false so the
        // partition point is 0. `0 - 1` used to panic here.
        let lines = [LyricLine { at: f64::NAN, text: "x".into() }];
        assert_eq!(active_index(&lines, 3.0), None);
    }

    #[test]
    fn next_boundary_never_returns_a_gap_that_cannot_be_a_duration() {
        // `tui`'s `poll_timeout` turns this into a sleep, and both of these
        // panicked `Duration::from_secs_f64` -- in release as well as debug.
        let nan = [LyricLine { at: f64::NAN, text: "x".into() }];
        assert_eq!(next_boundary(&nan, 3.0), None);
        let huge = [LyricLine { at: 6e45, text: "x".into() }];
        assert_eq!(next_boundary(&huge, 3.0), None);
    }

    #[test]
    fn next_boundary_counts_down_then_ends() {
        let l = parse_lrc("[00:10.00]a\n[00:20.00]b");
        assert!((next_boundary(&l, 0.0).unwrap() - 10.0).abs() < 1e-9);
        assert!((next_boundary(&l, 15.0).unwrap() - 5.0).abs() < 1e-9);
        assert_eq!(next_boundary(&l, 20.0), None, "on the last line");
        assert_eq!(next_boundary(&l, 99.0), None);
        assert_eq!(next_boundary(&[], 1.0), None);
    }
}
