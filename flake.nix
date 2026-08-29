{
  description = "yt-music-tui dev shell — libmpv, yt-dlp, and the native build deps reqwest/mpv pull in";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        inherit (pkgs) lib stdenv;

        # `ytm-core/build.rs` finds libmpv via pkg-config (see LIBMPV_DIR /
        # pkg-config note in CLAUDE.md). mpv-unwrapped is the derivation that
        # actually builds libmpv.so + mpv.pc + headers — the wrapped `mpv`
        # package adds a runtime script wrapper this shell has no use for.
        libmpv = pkgs.mpv-unwrapped;
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
            cmake # aws-lc-sys (reqwest 0.13's TLS backend) builds AWS-LC's C sources
          ] ++ lib.optionals stdenv.isx86_64 [
            nasm # aws-lc-sys/ring's hand-optimised asm on x86_64
          ];

          buildInputs = with pkgs; [
            libmpv
            yt-dlp
            ffmpeg # what yt-dlp shells out to for remuxing/extraction
            openssl # native-tls's Linux backend (openssl-sys is in Cargo.lock)
          ] ++ lib.optionals stdenv.isLinux [
            # media/mpris.rs is pure-Rust (zbus) and needs no libdbus at all;
            # playerctl is just the CLI CLAUDE.md's Commands section drives it with.
            playerctl
          ] ++ lib.optionals stdenv.isDarwin (with pkgs.darwin.apple_sdk.frameworks; [
            # media/nowplaying.rs (objc2 + objc2-app-kit/-media-player) links
            # against these; native-tls on Darwin uses Security.framework too.
            Foundation
            AppKit
            MediaPlayer
            Security
            CoreFoundation
          ]) ++ lib.optionals stdenv.isDarwin [
            libiconv
          ];

          # libmpv2-sys' build.rs probes this via pkg-config; set explicitly
          # rather than relying on mkShell's automatic buildInputs hook.
          PKG_CONFIG_PATH = "${libmpv}/lib/pkgconfig";

          shellHook = ''
            echo "yt-music-tui dev shell — rustc $(rustc --version 2>/dev/null || echo '?'), libmpv $(pkg-config --modversion mpv 2>/dev/null || echo '?'), yt-dlp $(yt-dlp --version 2>/dev/null || echo '?')"
          '';
        };
      });
}
