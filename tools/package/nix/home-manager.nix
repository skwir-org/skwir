# Home Manager's `programs.skwir` (ADR-055): skwir installed, and the
# settings declared merged into $XDG_CONFIG_HOME/skwir/settings.toml at
# each activation, as Home Manager's `mutableSettings` modules do. skwir
# writes that file too (its settings dialog, the window's layout) and
# takes in what changes there while it runs, so the file stays writable:
# a link into the store would be replaced at skwir's next save.
self: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.programs.skwir;
  toml = pkgs.formats.toml {};
  # The merger behind Home Manager's own `mutableSettings`: recent
  # releases only.
  merges = lib.hm.generators ? mkImpureConfigMerger;
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
        Settings merged into {file}`$XDG_CONFIG_HOME/skwir/settings.toml`
        at each activation. The values declared win, arrays included; what
        skwir or you set there otherwise is kept, the window's layout
        among it. Removing a declaration does not remove it from the file.
        A running skwir takes the change in at once, a setting that needs
        a restart excepted. The keys are those skwir writes there: a table
        per owner (`core`, `ui`, `reading`, a plugin's id); the search of
        skwir's Settings finds a setting by its key.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.settings == {} || merges;
        message = "programs.skwir.settings needs a Home Manager with lib.hm.generators.mkImpureConfigMerger (its mutable settings), newer than this one.";
      }
    ];

    home.packages = lib.mkIf (cfg.package != null) [cfg.package];

    home.activation.skwirSettings = lib.mkIf (cfg.settings != {} && merges) (
      lib.hm.dag.entryAfter ["linkGeneration"] (
        lib.hm.generators.mkImpureConfigMerger {
          inherit pkgs;
          format = "toml";
          empty = "{}";
          jqOperation = "$dynamic * $static";
          path = "${config.xdg.configHome}/skwir/settings.toml";
          staticSettings = toml.generate "skwir-settings.toml" cfg.settings;
          mode = "600";
        }
      )
    );
  };
}
