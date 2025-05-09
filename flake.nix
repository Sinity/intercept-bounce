{
  description = "Interception-bounce (debounce filter for Interception-Tools)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    devshell.url = "github:numtide/devshell";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    devshell,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {inherit system;};
      pname = "intercept-bounce";

      # ───── DON'T shadow pkgs.cargo ─────
      cargoToml = pkgs.lib.importTOML ./Cargo.toml;
      version = cargoToml.package.version;
    in {
      # ────────────────── build package ──────────────────
      packages = {
        ${pname} = pkgs.rustPlatform.buildRustPackage {
          inherit pname version;
          src = self;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [pkgs.pkg-config pkgs.openssl.dev];
          buildInputs = [pkgs.openssl];

          preBuild = ''
            cargo run --package xtask -- generate-docs
          '';

          postInstall = ''
            installManPage docs/man/intercept-bounce.1
            installShellCompletion --bash        docs/completions/intercept-bounce.bash
            installShellCompletion --zsh         docs/completions/_intercept-bounce
            installShellCompletion --fish        docs/completions/intercept-bounce.fish
            installShellCompletion --powershell  docs/completions/intercept-bounce.ps1
            installShellCompletion --nu          docs/completions/intercept-bounce.nu
          '';

          meta = with pkgs.lib; {
            description = "Interception-Tools bounce filter with statistics";
            license = licenses.mit;
            maintainers = [maintainers.sinity];
          };
        };
        
        default = self.packages.${system}.${pname};
      };

      # ────────────────── dev shell (devshell.mkShell) ──────────────────
      devShells.default = devshell.legacyPackages.${system}.mkShell {
        name = "intercept-bounce-dev";

        ## tools on $PATH inside the shell
        packages = with pkgs; [
          rustc
          cargo
          clippy
          rustfmt
          rust-analyzer
          nixpkgs-fmt
          pre-commit
          cargo-nextest
          cargo-fuzz
          cargo-audit
          cargo-udeps
          gdb
          interception-tools
          openssl.dev
          man-db
          git
          gh
        ];

        ## little helper aliases visible via "menu"
        commands = [
          {
            name = "xt";
            command = "cargo xtask";
            help = "Run xtask helper";
          }
          {
            name = "cl";
            command = "cargo clippy";
            help = "Clippy lints";
          }
          {
            name = "cf";
            command = "cargo fmt";
            help = "Format code";
          }
          {
            name = "ct";
            command = "cargo test";
            help = "Run tests";
          }
          {
            name = "nt";
            command = "cargo nextest run";
            help = "Parallel tests";
          }
          {
            name = "ca";
            command = "cargo audit";
            help = "Audit dependencies";
          }
          {
            name = "cu";
            command = "cargo udeps";
            help = "Detect unused deps";
          }
          {
            name = "fuzz";
            command = "cargo fuzz run";
            help = "Run fuzz targets";
          }
        ];

        ## banner printed once per interactive session
        motd = ''
          🛠  intercept-bounce dev shell
          Build:  cargo build [--release]    Tests: cargo nextest run (alias: nt)
          Pre-commit hooks: rustfmt · clippy · nixpkgs-fmt
          CI workflow: .github/workflows/ci.yml
        '';

        ## hook scripts
        devshell = {
          startup.pre-commit-hooks = {
            text = ''
              if [ -d .git ] && ! git config core.hooksPath &>/dev/null; then
                pre-commit install --install-hooks
              fi
            '';
          };
          
          # Display logs info at shell startup
          startup.logs = {
            text = ''
              echo "Logs: RUST_LOG=$RUST_LOG"
            '';
          };
        };

        # Environment variables
        env = [
          {
            name = "RUST_LOG";
            value = "\${RUST_LOG:-intercept_bounce=debug,warn}";
          }
        ];
      };
    });
}
