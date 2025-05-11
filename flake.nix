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

      # Fetch pre-commit-hooks repository source
      preCommitHooksSrc = pkgs.fetchFromGitHub {
        owner = "pre-commit";
        repo = "pre-commit-hooks";
        rev = "v5.0.0";
        sha256 = "sha256-BYNi/xtdichqsn55hqr1MSFwWpH+7cCbLfqmpn9cxto=";
      };
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
            pkgs.git
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
          gitleaks # gitleaks binary for local hook
          pre-commit
          interception-tools
          openssl
          man-db
          git
          gh
          gcc
          pkg-config
          yq-go # For modifying YAML in CI
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

      checks = {
        pre-commit-check = pkgs.stdenv.mkDerivation {
          name = "pre-commit-check";
          src = self;
          buildInputs = [self.devShells.${system}.default];
          phases = ["unpackPhase" "runPhase"];
          runPhase = ''
            set -x # Enable command tracing

            echo "Running pre-commit checks in $PWD"
            ls -la

            if [ ! -d ".git" ]; then
              echo "Initializing a temporary Git repository for pre-commit..."
              git init -b main
              git config user.email "ci@example.com"
              git config user.name "CI Bot"
              git add .
            fi

            if [ ! -f .pre-commit-config.yaml ]; then
              echo "ERROR: .pre-commit-config.yaml not found in $PWD"
              exit 1
            fi

            export PRE_COMMIT_HOME="$TMPDIR/pre-commit-cache"
            mkdir -p "$PRE_COMMIT_HOME"
            echo "PRE_COMMIT_HOME set to $PRE_COMMIT_HOME"

            # Create a CI-specific pre-commit config
            cp .pre-commit-config.yaml ci-pre-commit-config.yaml

            # Make the fetched pre-commit-hooks source path available to yq
            export PRE_COMMIT_HOOKS_SRC="${preCommitHooksSrc}"

            # Modify pre-commit-hooks repo to use the local Nix-fetched source
            yq -i '.repos[] |= select(.repo == "https://github.com/pre-commit/pre-commit-hooks").repo = env(PRE_COMMIT_HOOKS_SRC)' ci-pre-commit-config.yaml

            # Modify gitleaks hook to be a local system hook
            # 1. Delete the remote gitleaks repo entry
            yq -i 'del(.repos[] | select(.repo == "https://github.com/gitleaks/gitleaks"))' ci-pre-commit-config.yaml
            # 2. Add a new local gitleaks entry that uses the gitleaks binary from PATH
            cat >> ci-pre-commit-config.yaml << EOF
            - repo: local
              hooks:
                - id: gitleaks
                  name: Detect hardcoded secrets (gitleaks - system)
                  entry: gitleaks protect --verbose --redact --source=.
                  language: system
                  pass_filenames: false
                  types: [text]
            EOF

            echo "--- Using CI pre-commit config: ---"
            cat ci-pre-commit-config.yaml
            echo "---------------------------------"

            pre-commit run --config ci-pre-commit-config.yaml --all-files --show-diff-on-failure --verbose
            touch $out # Signal success
          '';
        };
      };
    });
}
