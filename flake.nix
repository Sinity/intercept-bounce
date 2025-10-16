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
      # This section was for the previous pre-commit setup
      # Keeping just the package name definition
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
            echo "Explicitly generating documentation..."
            cargo run --package xtask --bin xtask -- generate-docs
            echo "Documentation generation complete."
          '';

          postInstall = ''
            # Find and install the man page
            if [ -f "$out/share/doc/intercept-bounce/man/intercept-bounce.1" ]; then
              installManPage "$out/share/doc/intercept-bounce/man/intercept-bounce.1"
            elif [ -f "docs/man/intercept-bounce.1" ]; then
              installManPage "docs/man/intercept-bounce.1"
            fi

            # Install shell completions
            for shell in bash zsh fish; do
              file_name="intercept-bounce.$shell"
              if [ -f "$out/share/doc/intercept-bounce/completions/$file_name" ]; then
                installShellCompletion --$shell "$out/share/doc/intercept-bounce/completions/$file_name"
              elif [ -f "docs/completions/$file_name" ]; then
                installShellCompletion --$shell "docs/completions/$file_name"
              fi
            done

            # Install additional shell completions manually
            for shell in powershell nushell elvish; do
              ext=$([ "$shell" = "powershell" ] && echo "ps1" || [ "$shell" = "nushell" ] && echo "nu" || echo "elv")
              file_name="intercept-bounce.$ext"
              target_dir="$out/share/$shell/completions"

              if [ -f "$out/share/doc/intercept-bounce/completions/$file_name" ] || [ -f "docs/completions/$file_name" ]; then
                mkdir -p "$target_dir"

                if [ -f "$out/share/doc/intercept-bounce/completions/$file_name" ]; then
                  cp "$out/share/doc/intercept-bounce/completions/$file_name" "$target_dir/"
                elif [ -f "docs/completions/$file_name" ]; then
                  cp "docs/completions/$file_name" "$target_dir/"
                fi
              fi
            done
          '';

          meta = with pkgs.lib; {
            description = "Interception-Tools bounce filter with statistics";
            license = [
              licenses.mit
              licenses.asl20
            ];
            maintainers = [
              maintainers.sinity
            ];
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
            command = "cargo run --package xtask --bin xtask -- \"$@\"";
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

          Build:          cargo build [--release]
          Tests:          cargo nextest run (alias: nt)
          Documentation:  cargo xtask docs
          Development:    ./dev.sh [command]
          CI workflow:    .github/workflows/ci.yml
        '';
      };

      checks = {
        # rust-checks removed
      };
    })
    // {
      nixosModules = {
        default = import ./nix/modules/intercept-bounce.nix { inherit self; };
        intercept-bounce = import ./nix/modules/intercept-bounce.nix { inherit self; };
      };
    };
}
