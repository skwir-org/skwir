{
  description = "skwir — experimental GPU-accelerated file manager in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    # Only for checking the Home Manager module (`checks.home-manager`).
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
    home-manager,
  }:
    {
      # `programs.skwir` for Home Manager (ADR-055): skwir installed, its
      # settings merged into the file skwir writes too.
      homeManagerModules.default = import ./tools/package/nix/home-manager.nix self;
      overlays.default = final: _: {
        skwir = self.packages.${final.stdenv.hostPlatform.system}.default;
      };
    }
    # Linux, and Apple's ARM Macs for macOS work (Phase 5b): nixpkgs has
    # dropped Intel Macs.
    // flake-utils.lib.eachSystem ["x86_64-linux" "aarch64-linux" "aarch64-darwin"] (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [rust-overlay.overlays.default];
      };
      inherit (pkgs) lib stdenv;

      # The toolchain rust-toolchain.toml pins, which rustup follows too,
      # with the components the editor and CI need. The Windows and macOS
      # targets let `just check-cross` compile and lint those builds from
      # Linux (Phase 5): Zig is their C compiler, so the C in ring,
      # mimalloc or psm needs neither Apple's nor Microsoft's SDK.
      # WebAssembly builds the plugins that run as modules (ADR-046).
      pinned = (builtins.fromTOML (builtins.readFile ./rust-toolchain.toml)).toolchain.channel;
      rustToolchain = pkgs.rust-bin.stable.${pinned}.default.override {
        extensions = ["rust-src" "rust-analyzer" "clippy" "rustfmt" "llvm-tools"];
        targets = ["x86_64-pc-windows-gnu" "aarch64-apple-darwin" "wasm32-unknown-unknown"];
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

      # Optional native workers link these; skwir itself does not.
      mediaPlugins = with pkgs.gst_all_1; [
        gst-plugins-base
        gst-plugins-good
        gst-plugins-bad
        gst-plugins-ugly
        gst-libav
      ];
      buildInputs =
        [pkgs.libarchive pkgs.poppler_gi pkgs.djvulibre pkgs.cairo pkgs.glib pkgs.gst_all_1.gstreamer]
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
          gh # uploading verified local bundles to GitHub draft releases
          python3 # synthetic native-reader checks and comparisons
          cargo-nextest
          cargo-watch
          bacon
          cargo-flamegraph
          cargo-deny
          cargo-about
          cargo-edit
          cargo-outdated
          hyperfine
          fd
          ripgrep
          jq
          vulkan-tools # vulkaninfo for the toolkit spike
          pandoc # Office documents read as text, without LibreOffice
          djvulibre # the DjVu checks' fixtures (cjb2, c44, djvused, djvm)
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
          pacman # makepkg generates the AUR's .SRCINFO; builds stay in Arch
          git
          openssh # publishing the AUR checkout over SSH
          curl # downloading the release source for its AUR checksum
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
        # Nixpkgs installs this beside pacman, but makepkg defaults to /etc.
        # Only metadata is generated on the host; package builds use Arch's root.
        MAKEPKG_CONF = lib.optionalString stdenv.hostPlatform.isLinux "${pkgs.pacman}/etc/makepkg.conf";

        # wgpu/winit dlopen these at runtime; make them findable from `cargo run`.
        LD_LIBRARY_PATH = lib.optionalString stdenv.hostPlatform.isLinux (lib.makeLibraryPath linuxRuntimeLibs);

        shellHook = ''
          # Fast linking on Linux via mold, without touching the repo's .cargo/config.
          ${lib.optionalString stdenv.hostPlatform.isLinux ''
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="clang"
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"
          ''}
          echo "skwir dev shell: $(rustc --version) | $(cargo --version)"
        '';
      };

      formatter = pkgs.nixpkgs-fmt;
      # Optional native reader runtime; the default shell keeps its dependency
      # footprint. PDFium is dynamically loaded only by skwir-pdf-worker.
      devShells.pdf = self.devShells.${system}.default.overrideAttrs (_: {
        SKWIR_PDFIUM_LIBRARY = "${pkgs.pdfium}/lib/libpdfium${stdenv.hostPlatform.extensions.sharedLibrary}";
      });
      # skwir and its helpers as `just dist` builds them, for Linux
      # (Phase 5, step K): the helpers beside skwir, where it finds them,
      # what wgpu and GStreamer load at run time on its path, and its
      # desktop entry, metainfo and icon, as the other packages install
      # them. Built from the git tree: only tracked files are its source.
      # The tests need a session and fixtures of their own (`just
      # test-all`), not run here.
      packages.default = let
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
      in
        rustPlatform.buildRustPackage {
          pname = "skwir";
          version = "0.1.0";
          src = self;
          cargoLock = {
            lockFile = ./Cargo.lock;
            # The git sources Zed pins (ADR-064), fetched at the commit the
            # lock file names: no hash to keep beside each.
            allowBuiltinFetchGit = true;
          };
          # The programs shipped, as every package lists them.
          cargoBuildFlags =
            map (p: "--package=${p}")
            (lib.filter (p: p != "") (lib.splitString "\n" (builtins.readFile ./tools/package/programs)));
          doCheck = false;
          inherit buildInputs;
          nativeBuildInputs = [pkgs.pkg-config pkgs.cmake pkgs.clang pkgs.makeWrapper];
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          postInstall = lib.optionalString stdenv.hostPlatform.isLinux ''
            install -Dm644 tools/package/linux/org.skwir.skwir.desktop -t $out/share/applications
            install -Dm644 tools/package/linux/org.skwir.skwir.metainfo.xml -t $out/share/metainfo
            install -Dm644 crates/ui/assets/org.skwir.skwir.svg -t $out/share/icons/hicolor/scalable/apps
          '';
          postFixup = lib.optionalString stdenv.hostPlatform.isLinux ''
            wrapProgram $out/bin/skwir \
              --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath linuxRuntimeLibs} \
              --set-default GST_PLUGIN_SYSTEM_PATH_1_0 ${lib.makeSearchPath "lib/gstreamer-1.0" mediaPlugins}
          '';
          meta = {
            # The words every package says skwir in.
            inherit ((builtins.fromTOML (builtins.readFile ./crates/skwir/Cargo.toml)).package) description;
            # skwir's own; the Poppler, DjVuLibre and MuPDF readers, which
            # link libraries under the GPL and the AGPL (ADR-042, ADR-061).
            license = with lib.licenses; [asl20 gpl3Only agpl3Plus];
            mainProgram = "skwir";
          };
        };

      # For running the Windows build's tests under Wine (Phase 5), and
      # making its package on Linux (ADR-074): llvm-rc embeds skwir.exe's
      # icon and gpui's manifest, msitools' wixl makes the installer, and
      # icoutils' wrestool takes the icon back out for it. Wine
      # is not Windows: it answers for the logic (paths, identity, records)
      # and the package's programs and installer, never for the shell's
      # behaviour. Kept out of the default shell, whose closure it would
      # grow by about 1.5 GB.
      devShells.windows = self.devShells.${system}.default.overrideAttrs (old: {
        nativeBuildInputs =
          old.nativeBuildInputs
          ++ lib.optionals stdenv.hostPlatform.isLinux [pkgs.wine64Packages.stable pkgs.llvmPackages.llvm pkgs.msitools pkgs.icoutils];
      });

      # xmac (github.com/Jarred-Sumner/xmac, MIT or Apache-2.0) downloads
      # Apple's macOS SDK from Apple's software-update servers, for building
      # for macOS from Linux (`just macos-sdk`). Run by Bun from the source
      # of the commit pinned here: it talks to Apple's servers only and
      # starts xz, gzip and bzip2.
      packages.xmac = let
        src = pkgs.fetchFromGitHub {
          owner = "Jarred-Sumner";
          repo = "xmac";
          rev = "15a873d4b961b58ae812e6395e6b1636d02f88b5";
          hash = "sha256-KtfWC4x6t9TtQmzabYFjdH+SvSWjfn6u32zfsRFVVxo=";
        };
      in
        pkgs.writeShellApplication {
          name = "xmac";
          runtimeInputs = [pkgs.bun pkgs.xz pkgs.gzip pkgs.bzip2];
          text = ''exec bun ${src}/xmac.ts "$@"'';
        };

      # For building for macOS from Linux: xmac, its downloads kept in the
      # cache rather than the folder it runs in (its default). Kept out of
      # the default shell, which needs no Bun.
      devShells.macos = self.devShells.${system}.default.overrideAttrs (old: {
        nativeBuildInputs = old.nativeBuildInputs ++ [self.packages.${system}.xmac];
        shellHook =
          old.shellHook
          + ''
            export XMAC_CACHE_DIR="''${XMAC_CACHE_DIR:-$HOME/.cache/xmac}"
          '';
      });

      # The Home Manager module (`programs.skwir`), without building skwir:
      # the merge of mutable settings run on a file as skwir writes it
      # (what is declared wins, the rest stays, a second activation changes
      # nothing, a new file is its owner's alone); read-only settings linked
      # as the merger writes them; and the settings checked by the skwir
      # installed, here one that refuses "sepia", failing the build.
      checks = lib.optionalAttrs stdenv.hostPlatform.isLinux {
        home-manager = let
          home = skwir:
            (home-manager.lib.homeManagerConfiguration {
              inherit pkgs;
              modules = [
                self.homeManagerModules.default
                {
                  home = {
                    username = "someone";
                    homeDirectory = "/home/someone";
                    stateVersion = "25.11";
                  };
                  programs.skwir = {enable = true;} // skwir;
                }
              ];
            })
            .config;
          declared = {
            core.startup = "last";
            ui.theme = "dark";
            plugins.control.enabled = true;
          };
          mutable = home {
            package = null;
            mutableSettings = true;
            settings = declared;
          };
          linked = home {
            package = null;
            settings = declared;
          };
          refusing = pkgs.writeShellApplication {
            name = "skwir";
            runtimeInputs = [pkgs.jaq pkgs.gnugrep];
            text = ''
              file="$(printf '%s' "$3" | jaq -r .file)"
              if grep -q sepia "$file"; then
                echo "ui.theme: choose one of auto, light, dark" >&2
                exit 1
              fi
            '';
          };
          checked = theme:
            (home {
              package = refusing;
              settings.ui.theme = theme;
            })
            .xdg
            .configFile
            ."skwir/settings.toml"
            .source;
          merge = pkgs.writeText "skwir-settings-merge" mutable.home.activation.skwirSettings.data;
          refused = pkgs.testers.testBuildFailure (checked "sepia");
        in
          pkgs.runCommand "skwir-home-manager-check" {nativeBuildInputs = [pkgs.jaq];} ''
            errorEcho() { echo "$@" >&2; }
            export -f errorEcho
            config=$TMPDIR/home/.config/skwir
            sed "s|/home/someone|$TMPDIR/home|g" ${merge} > merge.sh
            settled() { jaq --from toml -c "$1" "$config/settings.toml"; }

            # Made where there is none, for its owner alone.
            bash -eu merge.sh
            [ "$(stat -c %a "$config/settings.toml")" = 600 ]
            [ "$(settled .ui.theme)" = '"dark"' ]

            # As skwir writes it: declared values win, the rest stays.
            printf '%s\n' '[core]' 'last_location = "/example/folder"' \
              '[ui]' 'theme = "light"' 'date_format = "iso"' \
              > "$config/settings.toml"
            bash -eu merge.sh
            [ "$(settled .ui.theme)" = '"dark"' ]
            [ "$(settled .core.startup)" = '"last"' ]
            [ "$(settled .plugins.control.enabled)" = true ]
            [ "$(settled .ui.date_format)" = '"iso"' ]
            [ "$(settled .core.last_location)" = '"/example/folder"' ]

            # Again, nothing changes.
            cp "$config/settings.toml" once.toml
            bash -eu merge.sh
            cmp once.toml "$config/settings.toml"

            # Read-only: linked as the merger would write it, and a writable
            # file left as declared gives way to the link.
            [ "$(jaq --from toml -c .ui.theme ${linked.xdg.configFile."skwir/settings.toml".source})" = '"dark"' ]
            [ -n ${lib.escapeShellArg linked.home.activation.skwirSettingsCleanup.data} ]

            # Checked by the skwir installed: what it refuses fails the build.
            [ "$(jaq --from toml -c .ui.theme ${checked "light"})" = '"light"' ]
            grep -q "choose one of auto, light, dark" ${refused}/testBuildFailure.log
            touch $out
          '';
      };
    });
}
