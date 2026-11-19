# Home Manager's `programs.skwir` (ADR-055, ADR-056): skwir installed,
# and the settings declared checked by skwir itself, then either linked
# read-only from the store (as Home Manager's modules do by default) or,
# with `mutableSettings`, merged into the writable file at each
# activation. skwir keeps the window's state, and what the user makes in it
# (saved searches, pinned tags), in files of their own, so a read-only
# settings file costs only the dialog's changes, which it says are not
# kept; it takes in a change of the file while it runs. On request, skwir
# is other programs' Open and Save dialogs (`fileChooser`, ADR-141) and
# what opens folders for them (`defaultFileManager`, ADR-140), through
# Home Manager's own `xdg.portal` and `xdg.mimeApps`.
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

    fileChooser = {
      enable = lib.mkEnableOption ''
        skwir as other programs' Open and Save dialogs, those that ask the
        desktop portal for one (ADR-141): the portal's configuration
        ({option}`xdg.portal.config`) names skwir's back end for the file
        chooser, and its `.portal` file and the session bus's service file
        for it are linked under {file}`$XDG_DATA_HOME`, where the portal and
        the bus look whatever profile they read. It applies when the portal
        starts again, the next login. Needs {option}`xdg.portal.enable`; a
        file chooser already named there for the same desktop is a
        conflict to remove. skwir's Settings then show the choice as kept
        by Home Manager'';

      desktops = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = ["common"];
        example = ["hyprland"];
        description = ''
          The desktops whose portal configuration names skwir for the file
          chooser: `xdg.portal.config.<desktop>` (`common` is
          {file}`portals.conf`). The portal reads a desktop's own file
          ({file}`hyprland-portals.conf`) before {file}`portals.conf` in one
          folder, so where {option}`xdg.portal.config` has a desktop's
          entry, name that desktop here.
        '';
      };
    };

    defaultFileManager = lib.mkOption {
      type = lib.types.bool;
      default = false;
      example = true;
      description = ''
        Whether skwir opens folders for every program: the desktop's
        default for `inode/directory` in {option}`xdg.mimeApps`, first
        before any other declared. skwir then also shows the files other
        programs show in their folder (their Show in Folder, ADR-140).
        Needs {option}`xdg.mimeApps.enable`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !(declared && cfg.mutableSettings) || merges;
        message = "programs.skwir.mutableSettings needs a Home Manager with lib.hm.generators.mkImpureConfigMerger (its mutable settings), newer than this one.";
      }
      {
        assertion = !cfg.fileChooser.enable || (cfg.package != null && config.xdg.portal.enable);
        message = "programs.skwir.fileChooser.enable needs skwir installed (programs.skwir.package) and the desktop portal (xdg.portal.enable).";
      }
      {
        assertion = !cfg.defaultFileManager || config.xdg.mimeApps.enable;
        message = "programs.skwir.defaultFileManager needs xdg.mimeApps.enable: it is the desktop's default for folders there.";
      }
    ];

    # Other programs' Open and Save dialogs (ADR-141): skwir named for the
    # file chooser, its back end found where the portal and the bus look.
    xdg.portal = lib.mkIf cfg.fileChooser.enable {
      extraPortals = lib.optional (cfg.package != null) cfg.package;
      config = lib.genAttrs cfg.fileChooser.desktops (_: {
        "org.freedesktop.impl.portal.FileChooser" = "skwir";
      });
    };
    xdg.dataFile = lib.mkIf (cfg.fileChooser.enable && cfg.package != null) {
      "xdg-desktop-portal/portals/skwir.portal".source = "${cfg.package}/share/xdg-desktop-portal/portals/skwir.portal";
      "dbus-1/services/org.freedesktop.impl.portal.desktop.skwir.service".source = "${cfg.package}/share/dbus-1/services/org.freedesktop.impl.portal.desktop.skwir.service";
    };

    # What opens folders for every program (ADR-140), skwir first.
    xdg.mimeApps.defaultApplications = lib.mkIf cfg.defaultFileManager {
      "inode/directory" = lib.mkBefore ["org.skwir.skwir.desktop"];
    };

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
