{
  description = "Brick Borrow stock watcher: alerts when sets become available";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { self
    , nixpkgs
    , flake-utils
    ,
    }:
    flake-utils.lib.eachDefaultSystem
      (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages.brickborrow-watch = pkgs.rustPlatform.buildRustPackage {
          pname = "brickborrow-watch";
          version = "0.1.0";
          # Keeping build outputs out of the store keeps the copy quick.
          src = pkgs.lib.cleanSourceWith {
            src = pkgs.lib.cleanSource ./.;
            filter = path: _type: !builtins.elem (baseNameOf path) [ "target" "result" ];
          };
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.pkg-config ];

          meta = {
            description = "Polls the Brick Borrow catalogue and alerts when sets become available";
            mainProgram = "brickborrow-watch";
            license = pkgs.lib.licenses.mit;
          };
        };

        packages.default = self.packages.${system}.brickborrow-watch;

        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
          ];
        };
      })
    // {
      homeManagerModules.brickborrow-watch =
        { config
        , lib
        , pkgs
        , ...
        }:
        let
          cfg = config.services.brickborrow-watch;
          settingsFile = (pkgs.formats.toml { }).generate "brickborrow-watch.toml" cfg.settings;
          configFile = if cfg.configFile != null then cfg.configFile else settingsFile;
        in
        {
          options.services.brickborrow-watch = {
            enable = lib.mkEnableOption "the Brick Borrow stock watcher";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.brickborrow-watch;
              description = "Package providing the brickborrow-watch binary.";
            };

            interval = lib.mkOption {
              type = lib.types.str;
              default = "30m";
              description = "systemd OnUnitActiveSec interval between polls.";
            };

            settings = lib.mkOption {
              type = (pkgs.formats.toml { }).type;
              default = { };
              description = ''
                Watcher configuration, rendered to TOML. Secrets belong in
                environmentFile, not here: the generated file lands in the
                world-readable Nix store.
              '';
              example = lib.literalExpression ''
                {
                  ntfy = {
                    enabled = true;
                    topic = "brickborrow-abc123";
                  };
                }
              '';
            };

            configFile = lib.mkOption {
              type = lib.types.nullOr lib.types.path;
              default = null;
              description = ''
                Path to a hand-written config file, used instead of settings.
                Point this at a file outside the Nix store when the config
                itself is sensitive.
              '';
            };

            environmentFile = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              example = "%h/.config/brickborrow-watch/secrets.env";
              description = ''
                File holding secrets as KEY=value lines, for example
                BRICKBORROW_SMTP_PASSWORD. Read by systemd, never copied into
                the Nix store.
              '';
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.user.services.brickborrow-watch = {
              Unit.Description = "Check Brick Borrow for newly available sets";
              Service = {
                Type = "oneshot";
                ExecStart = "${lib.getExe cfg.package} --config ${configFile}";
              } // lib.optionalAttrs (cfg.environmentFile != null) {
                EnvironmentFile = cfg.environmentFile;
              };
            };

            systemd.user.timers.brickborrow-watch = {
              Unit.Description = "Poll Brick Borrow for newly available sets";
              Timer = {
                OnBootSec = "2m";
                OnUnitActiveSec = cfg.interval;
                Persistent = true;
              };
              Install.WantedBy = [ "timers.target" ];
            };
          };
        };
    };
}
