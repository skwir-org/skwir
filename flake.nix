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

      # Stable toolchain with the components the editor and CI need. The
      # Windows and macOS targets let `just check-cross` compile and lint
      # those builds from Linux (Phase 5): Zig is their C compiler, so the
      # C in ring, mimalloc or psm needs neither Apple's nor Microsoft's SDK.
      rustToolchain = pkgs.rust-bin.stable.latest.default.override {
        extensions = ["rust-src" "rust-analyzer" "clippy" "rustfmt" "llvm-tools"];
        targets = ["x86_64-pc-windows-gnu" "aarch64-apple-darwin"];
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
          pandoc # Office documents read as text, without LibreOffice
          zig # the C compiler of the Windows and macOS checks
          cargo-zigbuild
          # The type database the macOS and Windows packages carry,
          # compiled from vendor/shared-mime-info (ADR-037): the program
          # alone, so the shell's data folders keep the desktop's database.
          (writeShellScriptBin "update-mime-database" ''
            exec ${shared-mime-info}/bin/update-mime-database "$@"
          '')
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
      # bibli and its helpers as `just dist` builds them, for Linux
      # (Phase 5, step K): the helpers beside bibli, where it finds them,
      # and what wgpu and GStreamer load at run time on its path. Built
      # from the git tree: only tracked files are its source. The tests
      # need a session and fixtures of their own (`just test-all`), not run
      # here.
      packages.default = let
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
      in
        rustPlatform.buildRustPackage {
          pname = "bibli";
          version = "0.0.1";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          # The programs shipped, as every package lists them.
          cargoBuildFlags =
            map (p: "--package=${p}")
            (lib.filter (p: p != "") (lib.splitString "\n" (builtins.readFile ./tools/package/programs)));
          doCheck = false;
          inherit buildInputs;
          nativeBuildInputs = [pkgs.pkg-config pkgs.cmake pkgs.clang pkgs.makeWrapper];
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          postFixup = lib.optionalString stdenv.hostPlatform.isLinux ''
            wrapProgram $out/bin/bibli \
              --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath linuxRuntimeLibs} \
              --set-default GST_PLUGIN_SYSTEM_PATH_1_0 ${lib.makeSearchPath "lib/gstreamer-1.0" mediaPlugins}
          '';
          meta = {
            description = "An experimental file manager, GPU-drawn, for humans, scripts and agents";
            # bibli's own; the Poppler and MuPDF readers, which link
            # libraries under the GPL and the AGPL (ADR-042).
            license = with lib.licenses; [asl20 gpl3Only agpl3Plus];
            mainProgram = "bibli";
          };
        };

      # For running the Windows build's tests under Wine (Phase 5). Wine
      # is not Windows: it answers for the logic (paths, identity, records),
      # never for the shell's behaviour. Kept out of the default shell, whose
      # closure it would grow by about 1.5 GB.
      devShells.windows = self.devShells.${system}.default.overrideAttrs (old: {
        nativeBuildInputs = old.nativeBuildInputs ++ lib.optionals stdenv.hostPlatform.isLinux [pkgs.wine64Packages.stable];
      });
    });
}
