{
  description = "Interception-bounce (debounce filter for Interception-Tools)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    devshell.url = "github:numtide/devshell";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    { self
    , nixpkgs
    , flake-utils
    , devshell
    , rust-overlay
    , ...
    }:
    flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      rust-bin = pkgs.rust-bin;
      pname = "intercept-bounce";
      cargoToml = pkgs.lib.importTOML ./Cargo.toml;
      version = cargoToml.package.version;
    in
    {
      packages = {
        ${pname} = pkgs.rustPlatform.buildRustPackage {
          inherit pname version;
          src = self;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [
            pkgs.pkg-config
            pkgs.openssl.dev
            pkgs.installShellFiles
            pkgs.makeWrapper
          ];
          buildInputs = [ pkgs.openssl ];

          preBuild = ''
            cargo run --package xtask -- generate-docs
          '';

          postInstall = ''
            # installManPage docs/man/intercept-bounce.1
            # installShellCompletion --bash        docs/completions/intercept-bounce.bash
            # installShellCompletion --zsh         docs/completions/intercept-bounce.zsh
            # installShellCompletion --fish        docs/completions/intercept-bounce.fish
            # installShellCompletion --powershell  docs/completions/intercept-bounce.ps1
            # installShellCompletion --nu          docs/completions/intercept-bounce.nu
          '';

          meta = with pkgs.lib; {
            description = "Interception-Tools bounce filter with statistics";
            license = licenses.mit;
            maintainers = [ maintainers.sinity ];
          };
        };

        default = self.packages.${system}.${pname};
      };

      # ────────────────── dev shell (devshell.mkShell) ──────────────────
      devShells.default = devshell.legacyPackages.${system}.mkShell {
        name = "intercept-bounce-dev";

        packages = with pkgs; [
          (rust-bin.nightly.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
          })
          nixpkgs-fmt
          cargo-nextest
          cargo-fuzz
          cargo-audit
          cargo-udeps
          gdb
          gitleaks # For secrets scanning
          pre-commit # The pre-commit framework
          interception-tools
          openssl
          man-db
          git
          gh
          gcc
          pkg-config
        ];

        commands = [
          {
            name = "xt";
            command = "cargo run --package xtask -- \"$@\"";
            help = "Run xtask helper";
          }
          {
            name = "cl";
            command = "cargo clippy --workspace --all-targets \"$@\"";
            help = "Clippy lints";
          }
          {
            name = "cf";
            command = "cargo fmt --all \"$@\"";
            help = "Format code";
          }
          {
            name = "ct";
            command = "cargo test --workspace \"$@\"";
            help = "Run tests";
          }
          {
            name = "nt";
            command = "cargo nextest run --workspace \"$@\"";
            help = "Parallel tests";
          }
          {
            name = "ca";
            command = "cargo audit \"$@\"";
            help = "Audit dependencies";
          }
          {
            name = "cu";
            command = "cargo udeps --workspace --all-targets \"$@\"";
            help = "Detect unused deps";
          }
          {
            name = "fuzz";
            command = "cargo fuzz \"$@\"";
            help = "Run cargo-fuzz commands (e.g., list, run <target>, add <target>)";
          }
        ];

        motd = ''
          🛠  intercept-bounce dev shell
          Build:  cargo build [--release]    Tests: cargo nextest run (alias: nt)
          CI workflow: .github/workflows/ci.yml
        '';
      };

      # Add a check for pre-commit hooks
      checks.pre-commit-check = self.devShells.${system}.default.inputDerivation {
        name = "pre-commit-check";
        command = "pre-commit run --all-files --show-diff-on-failure";
      };
    });
}
