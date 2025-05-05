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
        # Read version from Cargo.toml
        cargoToml = pkgs.lib.importTOML ./Cargo.toml;
        version = cargoToml.package.version;
      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "intercept-bounce";
          inherit version; # Derive version from Cargo.toml
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
            nushell # Add nushell for completion generation/installation
          ];
          buildInputs = [pkgs.openssl]; # Runtime dependency

          # Phase to generate and install docs before the standard build
          preBuild = ''
            export OUT_DIR=$(pwd)/target/generated
            mkdir -p $OUT_DIR
            echo "Generating docs in $OUT_DIR using helper binary..."
            # Build the helper binary (dev profile is faster for this)
            cargo build --quiet --package intercept-bounce --bin generate-cli-files --features shell_nu
            # Run the helper binary
            $(pwd)/target/debug/generate-cli-files
            echo "Finished generating docs."
          '';

          # Phase to install generated files alongside the main binary
          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin
            cp target/${pkgs.stdenv.hostPlatform.config}/release/intercept-bounce $out/bin/
            runHook postInstall
            echo "Installing man page..."
            mkdir -p $out/share/man/man1
            cp target/generated/intercept-bounce.1 $out/share/man/man1/

            echo "Installing completions..."
            install_completion() {
              local shell=$1
              local ext=$2
              local dest_dir=$out/share/$shell/completions
              mkdir -p $dest_dir
              # Install completion file, renaming based on shell conventions.
              if [ "$shell" = "bash" ]; then
                cp target/generated/intercept-bounce.$ext $dest_dir/intercept-bounce
              elif [ "$shell" = "zsh" ]; then
                cp target/generated/intercept-bounce.$ext $dest_dir/_intercept-bounce
              else
                # Fish, Elvish, PowerShell use the filename as is.
                cp target/generated/intercept-bounce.$ext $dest_dir/intercept-bounce.$ext
              fi
              echo "Installed $shell completion to $dest_dir"
            }

            install_completion bash bash
            install_completion elvish elv
            install_completion fish fish
            install_completion powershell ps1
            install_completion zsh zsh
            # Only install Nushell completion if the file exists
            if [ -f target/generated/intercept-bounce.nu ]; then
              install_completion nu nu
            fi

            echo "Finished installing docs."
          '';

          meta = {
            description = "Interception Tools bounce filter with statistics";
            license = pkgs.lib.licenses.mit; # Or Apache-2.0? Check Cargo.toml
            maintainers = with pkgs.lib.maintainers; [sinity]; # Add your handle if desired
          };
        };

        devShells.default = pkgs.mkShell {
          name = "intercept-bounce-dev";
          buildInputs = with pkgs; [
            rustc
            cargo
            clippy
            rustfmt
            rust-analyzer

            # System dependencies needed for build/runtime
            pkg-config
            openssl

            nixpkgs-fmt # For formatting Nix files
          ];
        };
      }
    );
}
