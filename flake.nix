{
  description = "bibli — experimental GPU-accelerated file manager in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [rust-overlay.overlays.default];
      };
      inherit (pkgs) lib stdenv;

      # Stable toolchain with the components the editor and CI need.
      rustToolchain = pkgs.rust-bin.stable.latest.default.override {
        extensions = ["rust-src" "rust-analyzer" "clippy" "rustfmt" "llvm-tools"];
      };

      # Native libraries needed to build and run wgpu / GPUI / Floem on Linux.
      # Most of these are dlopen'ed at runtime, hence LD_LIBRARY_PATH below.
      linuxRuntimeLibs = with pkgs; [
        wayland
        libxkbcommon
        vulkan-loader
        libGL
        fontconfig
        freetype
        libx11
        libxcursor
        libxi
        libxrandr
        libxcb
      ];

      linuxBuildInputs = with pkgs;
        linuxRuntimeLibs
        ++ [
          openssl
          zlib
          expat
          dbus # udisks2 / zbus
          sqlite
        ];

      darwinBuildInputs = with pkgs;
        lib.optionals stdenv.hostPlatform.isDarwin [
          libiconv
        ];

      # Optional native workers link these; bibli itself does not.
      mediaPlugins = with pkgs.gst_all_1; [
        gst-plugins-base
        gst-plugins-good
        gst-plugins-bad
        gst-plugins-ugly
        gst-libav
      ];
      buildInputs =
        [pkgs.libarchive pkgs.poppler_gi pkgs.cairo pkgs.glib pkgs.gst_all_1.gstreamer]
        ++ mediaPlugins
        ++ (lib.optionals stdenv.hostPlatform.isLinux linuxBuildInputs) ++ darwinBuildInputs;

      nativeBuildInputs = with pkgs;
        [
          rustToolchain
          pkg-config
          cmake
          clang
          llvmPackages.libclang # bindgen
        ]
        ++ lib.optionals stdenv.hostPlatform.isLinux [mold];

      devTools = with pkgs;
        [
          just
          python3 # synthetic native-reader checks and comparisons
          cargo-nextest
          cargo-watch
          bacon
          cargo-flamegraph
          cargo-deny
          cargo-edit
          cargo-outdated
          hyperfine
          fd
          ripgrep
          jq
          vulkan-tools # vulkaninfo for the toolkit spike
        ]
        ++ lib.optionals stdenv.hostPlatform.isLinux [
          bubblewrap # isolated Office document previews
          samply
          heaptrack
          wayland-utils
        ];
    in {
      devShells.default = pkgs.mkShell {
        inherit buildInputs;
        nativeBuildInputs = nativeBuildInputs ++ devTools;

        LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        RUST_BACKTRACE = "1";
        GST_PLUGIN_SYSTEM_PATH_1_0 = lib.makeSearchPath "lib/gstreamer-1.0" mediaPlugins;

        # wgpu/winit dlopen these at runtime; make them findable from `cargo run`.
        LD_LIBRARY_PATH = lib.optionalString stdenv.hostPlatform.isLinux (lib.makeLibraryPath linuxRuntimeLibs);

        shellHook = ''
          # Fast linking on Linux via mold, without touching the repo's .cargo/config.
          ${lib.optionalString stdenv.hostPlatform.isLinux ''
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="clang"
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"
          ''}
          echo "bibli dev shell: $(rustc --version) | $(cargo --version)"
        '';
      };

      formatter = pkgs.nixpkgs-fmt;
      # Optional native reader runtime; the default shell keeps its dependency
      # footprint. PDFium is dynamically loaded only by bibli-pdf-worker.
      devShells.pdf = self.devShells.${system}.default.overrideAttrs (_: {
        BIBLI_PDFIUM_LIBRARY = "${pkgs.pdfium}/lib/libpdfium${stdenv.hostPlatform.extensions.sharedLibrary}";
      });
    });
}
