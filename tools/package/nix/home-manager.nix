# Home Manager's `programs.skwir` (ADR-055, ADR-056): skwir installed,
# and the settings declared checked by skwir itself, then either linked
# read-only from the store (as Home Manager's modules do by default) or,
# with `mutableSettings`, merged into the writable file at each
# activation. skwir keeps the window's state, and what the user makes in it
# (saved searches, pinned tags), in files of their own, so a read-only
# settings file costs only the dialog's changes, which it says are not
# kept; it takes in a change of the file while it runs.
self: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.programs.skwir;
  toml = pkgs.formats.toml {};
  # The merger behind Home Manager's own `mutableSettings`, and its
  # cleanup: recent releases only.
  merges = lib.hm.generators ? mkImpureConfigMerger;
  cleans = lib.hm.generators ? mkImpureConfigCleanup;
  declared = cfg.settings != {};
  generated = toml.generate "skwir-settings.toml" cfg.settings;
  # Written as the merger writes TOML, so that a writable file left as it
  # was declared is replaced by the link when mutableSettings is turned
  # off (`mkImpureConfigCleanup` compares the bytes).
  written = pkgs.runCommand "skwir-settings.toml" {nativeBuildInputs = [pkgs.jaq];} ''
    settings="$(jaq --from toml --to toml '.' ${generated})"
    printf '%s\n' "$settings" > "$out"
  '';
  # Checked by the skwir installed, with no window, against the settings
  # it and its plugins declare: a value a setting does not take, or a key
  # no setting has, fails the build before anything is switched.
  checked =
    if cfg.package == null || !cfg.checkSettings
    then written
    else
      pkgs.runCommand "skwir-settings-checked.toml" {} ''
        export HOME="$TMPDIR/home" XDG_RUNTIME_DIR="$TMPDIR/run"
        mkdir -p "$HOME" "$XDG_RUNTIME_DIR"
        ${lib.getExe cfg.package} cmd settings.check ${lib.escapeShellArg (builtins.toJSON {file = "${written}";})} > /dev/null
        cp ${written} "$out"
      '';
in {
  options.programs.skwir = {
    enable = lib.mkEnableOption "skwir, the file manager";

    package = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default or null;
      defaultText = lib.literalMD "skwir's flake package for the system";
      description = "The skwir package to install, or `null` to install none.";
    };

    settings = lib.mkOption {
      inherit (toml) type;
      default = {};
      example = lib.literalExpression ''
        {
          core.startup = "last";
          ui.theme = "dark";
          plugins.control.enabled = true;
        }
      '';
      description = ''
        skwir's settings, written to
        {file}`$XDG_CONFIG_HOME/skwir/settings.toml`: the keys are those
        skwir writes there, a table per owner (`core`, `ui`, `reading`, a
        plugin's id), and the search of skwir's Settings finds a setting by
        its key. They are checked by the skwir installed: a value a setting
        does not take, or a key no setting has, fails the build. The
        window's state ({file}`$XDG_STATE_HOME/skwir`) and what you make in
        skwir, as saved searches, pinned tags and tags' colours
        ({file}`$XDG_DATA_HOME/skwir`), are kept elsewhere, and are no
        settings to declare.
      '';
    };

    mutableSettings = lib.mkOption {
      type = lib.types.bool;
      default = false;
      example = true;
      description = ''
        Whether to merge the declared settings into a writable settings
        file at each activation, keeping what skwir's Settings or you
        change there. The values declared win, arrays included; removing a
        declaration does not remove it from the file. A running skwir takes
        the change in at once.

        The default links the file read-only from the store: skwir's
        Settings then say a change there is not kept. Switching back to it
        replaces the writable file only when it is as declared; a changed
        one meets Home Manager's usual collision handling.
      '';
    };

    checkSettings = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether the skwir installed checks the declared settings while the
        configuration builds, with no window. Off, a wrong value reaches
        the file, where skwir reads what it can of it.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !(declared && cfg.mutableSettings) || merges;
        message = "programs.skwir.mutableSettings needs a Home Manager with lib.hm.generators.mkImpureConfigMerger (its mutable settings), newer than this one.";
      }
    ];

    home.packages = lib.mkIf (cfg.package != null) [cfg.package];

    xdg.configFile."skwir/settings.toml" = lib.mkIf (declared && !cfg.mutableSettings) {
      source = checked;
    };

    home.activation.skwirSettings = lib.mkIf (declared && cfg.mutableSettings && merges) (
      lib.hm.dag.entryAfter ["linkGeneration"] (
        lib.hm.generators.mkImpureConfigMerger {
          inherit pkgs;
          format = "toml";
          empty = "{}";
          jqOperation = "$dynamic * $static";
          path = "${config.xdg.configHome}/skwir/settings.toml";
          staticSettings = checked;
          mode = "600";
        }
      )
    );

    home.activation.skwirSettingsCleanup = lib.mkIf (declared && !cfg.mutableSettings && cleans) (
      lib.hm.dag.entryBetween ["linkGeneration"] ["writeBoundary"] (
        lib.hm.generators.mkImpureConfigCleanup {
          file = config.xdg.configFile."skwir/settings.toml";
        }
      )
    );
  };
}
