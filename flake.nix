{
  description = "Debounce plugin for Interception Tools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem
    (
      system: let
        pkgs = import nixpkgs {inherit system;};
        cargoToml = pkgs.lib.importTOML ./Cargo.toml;
        version = cargoToml.package.version;
      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "intercept-bounce";
          inherit version;
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            pkg-config
            git # Needed by vergen build script if building outside git repo
            # Shells needed by clap_complete::generate_to in generate-cli-files
            bash
            elvish
            fish
            powershell
            zsh
            nushell
          ];
          buildInputs = [pkgs.openssl]; # Runtime dependency

          preBuild = ''
            echo "Generating docs using xtask..."
            cargo run --package xtask -- generate-docs
            echo "Finished generating docs."
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin
            cp target/${pkgs.stdenv.hostPlatform.config}/release/intercept-bounce $out/bin/
            runHook postInstall
            echo "Installing man page..."
            mkdir -p $out/share/man/man1
            cp docs/man/intercept-bounce.1 $out/share/man/man1/

            echo "Installing completions..."
            install_completion() {
              local shell=$1
              local ext=$2
              local dest_dir=$out/share/$shell/completions
              mkdir -p $dest_dir
              # Install completion file, renaming based on shell conventions.
              if [ "$shell" = "bash" ]; then
                cp docs/completions/intercept-bounce.$ext $dest_dir/intercept-bounce
              elif [ "$shell" = "zsh" ]; then
                cp docs/completions/intercept-bounce.$ext $dest_dir/_intercept-bounce
              else
                # Fish, Elvish, PowerShell use the filename as is.
                cp docs/completions/intercept-bounce.$ext $dest_dir/intercept-bounce.$ext
              fi
              echo "Installed $shell completion to $dest_dir"
            }

            install_completion bash bash
            install_completion elvish elv
            install_completion fish fish
            install_completion powershell ps1
            install_completion zsh zsh
            install_completion nu nu

            # Ensure all completions are installed
            echo "Verifying completion files were installed..."
            find $out/share -type f -name "*intercept-bounce*" | sort

            echo "Finished installing docs."
          '';

          meta = {
            description = "Interception Tools bounce filter with statistics";
            license = pkgs.lib.licenses.mit;
            maintainers = with pkgs.lib.maintainers; [sinity];
          };
        };

        devShells.default = pkgs.mkShell {
          name = "intercept-bounce-dev";
          buildInputs = with pkgs; [
            pkg-config
            openssl
            nixpkgs-fmt

            # Core Rust tooling
            rustc
            cargo
            clippy
            rustfmt
            rust-analyzer

            # Nix formatting
            nixpkgs-fmt

            # Fuzzing
            cargo-fuzz

            # Debugging
            gdb

            # Code analysis & quality
            cargo-audit
            cargo-udeps

            # Runtime dependencies for testing
            interception-tools

            # Documentation viewing
            man-db # Provides the 'man' command

            # Version control & GitHub CLI
            git
            gh

            # Direnv for automatic shell activation
            direnv
          ];

          # Commands run when entering the shell
          shellHook = ''
            # Set default log level for development
            export RUST_LOG="intercept_bounce=debug,warn"

            # Useful aliases
            alias xt="cargo run --package xtask --"
            alias cl="cargo clippy --all-targets --all-features -- -D warnings"
            alias cf="cargo fmt --all"
            alias ct="cargo test --all-targets --all-features"
            alias ca="cargo audit"
            alias cu="cargo udeps"
            alias fuzz="cargo fuzz"

            # Welcome message
            echo ""
            echo "Welcome to the intercept-bounce development shell!"
            echo ""
            echo "Useful aliases:"
            echo "  xt <task> : Run an xtask (e.g., xt generate-docs)"
            echo "  cl        : Run clippy (strict)"
            echo "  cf        : Run cargo fmt"
            echo "  ct        : Run cargo test"
            echo "  ca        : Run cargo audit"
            echo "  cu        : Run cargo udeps"
            echo "  fuzz      : Run cargo fuzz commands (e.g., fuzz list, fuzz run <target>)"
            echo ""
            echo "Tools available: cargo, rustc, clippy, rustfmt, rust-analyzer, gdb,"
            echo "  cargo-fuzz, cargo-audit, cargo-udeps, intercept, uinput, udevmon,"
            echo "  man, git, gh, direnv, nixpkgs-fmt, etc."
            echo ""
            echo "Run 'man ./docs/man/intercept-bounce.1' to view the man page."
            echo "Set RUST_LOG environment variable to override log level (current: $RUST_LOG)."
            echo ""
          '';
        };
      }
    );
}
