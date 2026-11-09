# skwir and its helpers, built as nixpkgs builds a package (ADR-124): one
# function, a flag for each helper that brings libraries of its own or that
# few want, and every flag's default the one `auto` uses (PDFium for PDFs,
# DjVuLibre for DjVu; Poppler and MuPDF only when chosen in the settings).
# The flake makes three of it: `default`, `minimal` (every flag off) and
# `full` (every flag on); `.override { withMedia = false; }` makes any other.
#
# Three arguments are the flake's own: `src` (its git tree), `version` (the
# workspace's) and `cargoLock` (its Cargo.lock, with the git sources Zed
# pins fetched as the lock names them, ADR-064). A copy in nixpkgs gives
# `fetchFromGitHub`, a literal version and `cargoHash` in their place; the
# rest is written to stand there as it is.
{
  lib,
  stdenv,
  rustPlatform,
  fetchurl,
  makeWrapper,
  pkg-config,
  cmake,
  python3,

  # What skwir draws and runs with; wgpu opens the GPU's, Wayland's and
  # X's libraries itself, through the program's own search path.
  wayland,
  libxkbcommon,
  vulkan-loader,
  libGL,
  fontconfig,
  freetype,
  libx11,
  libxcursor,
  libxi,
  libxrandr,
  libxcb,
  openssl,
  zlib,
  expat,
  dbus,
  sqlite,
  libiconv,

  # What the helpers link, each behind its flag.
  pdfium,
  poppler_gi,
  cairo,
  glib,
  djvulibre,
  libarchive,
  gst_all_1,

  # The flake's (see above).
  src,
  version,
  cargoLock,

  # PDFs read by PDFium, nixpkgs' (the reader `auto` takes first).
  withPdfium ? true,
  # Poppler's PDF reader, chosen in the settings.
  withPoppler ? false,
  # MuPDF's reader, chosen in the settings (AGPL, ADR-014); built from the
  # source its crate bundles, it links no library of the system's.
  withMupdf ? false,
  # DjVuLibre's DjVu reader (the built-in djvu-rs one stays, ADR-061).
  withDjvulibre ? true,
  # Archives opened and extracted (libarchive).
  withArchive ? true,
  # Mermaid diagrams drawn (pure Rust, but large).
  withMermaid ? true,
  # The audio and video player (GStreamer).
  withMedia ? true,
  # The player's formats, from this package's own GStreamer; its base
  # plugins are always there: the helper links them, and plays through
  # their `playbin`.
  gstPlugins ? (
    ps: with ps; [
      gst-plugins-base
      gst-plugins-good
      gst-plugins-bad
      gst-plugins-ugly
      gst-libav
    ]
  ),
}:

let
  # Each flag, what it builds and what that links: the one list the
  # variants are made from (`passthru.features`).
  features = {
    withPdfium = {
      on = withPdfium;
      programs = [ "skwir-pdf-worker" ];
      libraries = [ ];
      notices = [ ];
    };
    withPoppler = {
      on = withPoppler;
      programs = [ "skwir-poppler-worker" ];
      libraries = [
        poppler_gi
        cairo
        glib
      ];
      notices = [ "skwir-poppler" ];
    };
    withMupdf = {
      on = withMupdf;
      programs = [ "skwir-mupdf-worker" ];
      libraries = [ ];
      notices = [ "skwir-mupdf" ];
    };
    withDjvulibre = {
      on = withDjvulibre;
      programs = [ "skwir-djvulibre-worker" ];
      libraries = [ djvulibre ];
      notices = [ "skwir-djvulibre" ];
    };
    withArchive = {
      on = withArchive;
      programs = [ "skwir-archive-worker" ];
      libraries = [ libarchive ];
      notices = [ ];
    };
    withMermaid = {
      on = withMermaid;
      programs = [ "skwir-mermaid-worker" ];
      libraries = [ ];
      notices = [ "skwir-mermaid" ];
    };
    withMedia = {
      on = withMedia;
      programs = [ "skwir-media-worker" ];
      libraries = [
        gst_all_1.gstreamer
        gst_all_1.gst-plugins-base
      ];
      notices = [ "skwir-media" ];
    };
  };
  enabled = lib.filter (feature: feature.on) (lib.attrValues features);

  # The helpers every build carries: no library of their own.
  helpers = [
    "skwir-djvu-worker"
    "skwir-model-worker"
    "skwir-module-worker"
    "skwir-tree-sitter-worker"
    "skwir-print-worker"
  ]
  # KDE's kmtpd is Linux's alone (ADR-097).
  ++ lib.optional stdenv.hostPlatform.isLinux "skwir-kmtp-worker"
  ++ lib.concatMap (feature: feature.programs) enabled;

  plugins = lib.unique ([ gst_all_1.gst-plugins-base ] ++ gstPlugins gst_all_1);

  # What wgpu, winit and xkbcommon open at run time.
  drawing = [
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

  # Skwir's own fonts (ADR-118): the archives tools/package/fonts.lock pins,
  # from which fonts.py takes the static files.
  fontArchives = {
    "Inter-4.1.zip" = fetchurl {
      url = "https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip";
      sha256 = "9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e";
    };
    "JetBrainsMono-2.304.zip" = fetchurl {
      url = "https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip";
      sha256 = "6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf";
    };
  };
in
rustPlatform.buildRustPackage {
  pname = "skwir";
  inherit version src cargoLock;

  # What is shipped (`just dist`): release without the symbols kept for
  # flamegraphs.
  buildType = "dist";
  cargoBuildFlags = map (program: "--package=${program}") ([ "skwir" ] ++ helpers);
  # The tests need a session and fixtures of their own (`just test-all`).
  doCheck = false;

  nativeBuildInputs = [
    pkg-config
    cmake
    makeWrapper
  ]
  # MuPDF's crate makes its bindings with bindgen.
  ++ lib.optional withMupdf rustPlatform.bindgenHook;

  buildInputs =
    lib.optionals stdenv.hostPlatform.isLinux (
      drawing
      ++ [
        openssl
        zlib
        expat
        dbus
        sqlite
      ]
    )
    ++ lib.optional stdenv.hostPlatform.isDarwin libiconv
    ++ lib.concatMap (feature: feature.libraries) enabled;

  postInstall = ''
    # The helpers where skwir looks for them, off the PATH
    # (process::helper_folders: <prefix>/libexec/skwir).
    mkdir -p $out/libexec/skwir
    ${lib.concatMapStrings (helper: "mv $out/bin/${helper} $out/libexec/skwir/\n") helpers}

    mkdir -p fonts/.downloads
    ${lib.concatStrings (
      lib.mapAttrsToList (name: archive: "cp ${archive} fonts/.downloads/${name}\n") fontArchives
    )}
    ${python3.interpreter} tools/package/fonts.py fonts
    install -Dm644 fonts/fonts/* -t $out/share/skwir/fonts
    install -Dm644 LICENSE -t $out/share/licenses/skwir
    cp -R fonts/licenses/fonts $out/share/licenses/skwir/

    # The notices of what this build's programs link (ADR-063): skwir's
    # and its built-in helpers', then each helper's this build carries.
    cat ${
      lib.concatMapStringsSep " " (package: "tools/package/notices/${package}/THIRD-PARTY-NOTICES") (
        [ "skwir" ] ++ lib.concatMap (feature: feature.notices) enabled
      )
    } > $out/share/licenses/skwir/THIRD-PARTY-NOTICES
  ''
  + lib.optionalString stdenv.hostPlatform.isLinux ''
    install -Dm644 tools/package/linux/org.skwir.skwir.desktop -t $out/share/applications
    install -Dm644 tools/package/linux/org.skwir.skwir.metainfo.xml -t $out/share/metainfo
    install -Dm644 crates/ui/assets/org.skwir.skwir.svg -t $out/share/icons/hicolor/scalable/apps
  '';

  # What the window opens at run time, found through the program's own
  # search path rather than LD_LIBRARY_PATH, which every program skwir
  # starts would inherit; added after fixup has shrunk it to what is
  # linked. skwir's wrapper only names where PDFium is: a setting naming
  # another still wins. The player's plugins are told to its helper alone:
  # set on skwir, a video player started from it would take them for its
  # own, built against another GStreamer and glibc.
  postFixup =
    lib.optionalString stdenv.hostPlatform.isLinux ''
      patchelf --add-rpath ${lib.makeLibraryPath drawing} $out/bin/skwir
    ''
    + lib.optionalString withPdfium ''
      wrapProgram $out/bin/skwir --set-default SKWIR_PDFIUM_LIBRARY ${lib.getLib pdfium}/lib/libpdfium${stdenv.hostPlatform.extensions.sharedLibrary}
    ''
    + lib.optionalString withMedia ''
      wrapProgram $out/libexec/skwir/skwir-media-worker --set-default GST_PLUGIN_SYSTEM_PATH_1_0 ${lib.makeSearchPath "lib/gstreamer-1.0" plugins}
    '';

  # The toolchain stays out of what is installed: rustc writes the paths
  # of std's sources into panic messages when they are in its sysroot.
  disallowedReferences = [
    rustPlatform.rust.rustc
    rustPlatform.rust.cargo
  ];

  passthru.features = lib.attrNames features;

  meta = {
    description = "An augmented file manager, for humans and agents";
    homepage = "https://skwir.org";
    changelog = "https://github.com/skwir-org/skwir/blob/main/CHANGELOG.md";
    # skwir's own, with the model helper's STEP reader (ogeom) under the
    # LGPL; Poppler's and DjVuLibre's readers link libraries under the
    # GPL, MuPDF's under the AGPL (ADR-042, ADR-061).
    license =
      with lib.licenses;
      [
        asl20
        lgpl21Only
      ]
      ++ lib.optional (withPoppler || withDjvulibre) gpl3Only
      ++ lib.optional withMupdf agpl3Plus;
    mainProgram = "skwir";
  };
}
