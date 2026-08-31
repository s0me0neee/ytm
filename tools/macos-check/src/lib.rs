//! A stand-in for `ytm-core`, holding only what `media/nowplaying.rs` reaches
//! for — so that file can be type-checked against the real `objc2` bindings
//! from a machine with no Apple SDK. See this crate's `Cargo.toml` for why.
//!
//! Nothing here is shipped or run. If the backend starts using something else
//! out of `ytm-core`, the shape of it has to be added below, and keeping the
//! two in step is the price of being able to check the file at all.

pub mod player {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum PlayMode {
        #[default]
        Cycle,
        Single,
        Shuffle,
    }
}

pub mod cover {
    pub async fn fetch_bytes(_url: &str) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

pub mod media;
