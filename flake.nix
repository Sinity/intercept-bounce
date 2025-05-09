{
  description = "Interception-bounce (debounce filter for Interception-Tools)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    devshell.url = "github:numtide/devshell";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    devshell,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [rust-overlay.overlays.default];
      };
      rust-bin = pkgs.rust-bin;
      pname = "intercept-bounce";
      cargoToml = pkgs.lib.importTOML ./Cargo.toml;
      version = cargoToml.package.version;
    in {
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
          buildInputs = [pkgs.openssl];

          preBuild = ''
            echo "Generating documentation with xtask..."
            cargo run --package xtask -- generate-docs
            echo "Documentation generation complete. Listing generated files:"
            ls -l docs/man || echo "docs/man not found or ls failed"
            ls -l docs/completions || echo "docs/completions not found or ls failed"
          '';

          postInstall = ''
            echo "Starting postInstall phase..."

            echo "Installing man page..."
            installManPage docs/man/intercept-bounce.1

            echo "Installing Bash completion..."
            installShellCompletion --bash docs/completions/intercept-bounce.bash
            echo "Installing Zsh completion..."
            installShellCompletion --zsh docs/completions/intercept-bounce.zsh
            echo "Installing Fish completion..."
            installShellCompletion --fish docs/completions/intercept-bounce.fish

            echo "Installing PowerShell completion manually..."
            mkdir -p $out/share/powershell/completions
            cp docs/completions/intercept-bounce.ps1 $out/share/powershell/completions/

            echo "Installing Nushell completion manually..."
            mkdir -p $out/share/nushell/completions
            cp docs/completions/intercept-bounce.nu $out/share/nushell/completions/

            echo "Installing Elvish completion manually..."
            mkdir -p $out/share/elvish/completions
            cp docs/completions/intercept-bounce.elv $out/share/elvish/completions/

            echo "postInstall phase complete."
          '';

          meta = with pkgs.lib; {
            description = "Interception-Tools bounce filter with statistics";
            license = [licenses.mit licenses.asl20];
            maintainers = [maintainers.sinity];
          };
        };

        default = self.packages.${system}.${pname};
      };

      devShells.default = devshell.legacyPackages.${system}.mkShell {
        name = "intercept-bounce-dev";

        packages = with pkgs; [
          (rust-bin.nightly.latest.default.override {
            extensions = ["rust-src" "rust-analyzer" "clippy" "rustfmt"];
          })
          nixpkgs-fmt
          alejandra
          cargo-nextest
          cargo-fuzz
          cargo-audit
          cargo-udeps
          gdb
          gitleaks
          pre-commit
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

      checks.pre-commit-check = self.devShells.${system}.default.inputDerivation {
        name = "pre-commit-check";
        command = "pre-commit run --all-files --show-diff-on-failure";
      };
    });
}
