{
  description = "Debounce plugin for Interception Tools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  # Define outputs using flake-utils.lib.eachDefaultSystem
  # This iterates over common systems (x86_64-linux, aarch64-linux, etc.)
  # and applies the function provided to each system.
  outputs = {
    self,
    nixpkgs,
    flake-utils,
    ...
  } @ inputs:
    flake-utils.lib.eachDefaultSystem
    (
      system:
      # The 'system' variable is now in scope here
      let
        pkgs = import nixpkgs {inherit system;}; # Use the system variable
      in {
        # Define packages for this specific system
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "intercept-bounce";
          version = "0.6.0";
          src = ./.;
          cargoHash = "sha256-NGhaFLAdJzfCk0YZRVrNriqd+2W1Ohbbya4s3Jid+/8=";

          nativeBuildInputs = [
            pkgs.pkg-config
            pkgs.git # Needed for vergen build script
          ];
          buildInputs = [pkgs.openssl];

          # Phase to generate and install docs before the standard build
          preBuild = ''
            export OUT_DIR=$(pwd)/target/generated
            mkdir -p $OUT_DIR
            echo "Generating docs in $OUT_DIR using helper binary..."
            # Ensure the helper binary is built first
            cargo build --package intercept-bounce --bin generate-cli-files
            # Run the helper binary from the target directory
            $(pwd)/target/debug/generate-cli-files
            echo "Finished generating docs."
          '';

          # Phase to install generated files alongside the main binary
          installPhase = ''
            # Standard binary installation
            runHook preInstall
            mkdir -p $out/bin
            cp target/${pkgs.stdenv.hostPlatform.config}/release/intercept-bounce $out/bin/
            runHook postInstall

            # Install man page
            echo "Installing man page..."
            mkdir -p $out/share/man/man1
            cp target/generated/intercept-bounce.1 $out/share/man/man1/

            # Install completions
            echo "Installing completions..."
            install_completion() {
              local shell=$1
              local ext=$2
              local dest_dir=$out/share/$shell/completions
              mkdir -p $dest_dir
              # Install completion file, potentially renaming it based on shell conventions
              if [ "$shell" = "bash" ]; then
                cp target/generated/intercept-bounce.$ext $dest_dir/intercept-bounce
              elif [ "$shell" = "zsh" ]; then
                 # Zsh completions often start with an underscore
                cp target/generated/intercept-bounce.$ext $dest_dir/_intercept-bounce
              else
                cp target/generated/intercept-bounce.$ext $dest_dir/intercept-bounce.$ext
              fi
              echo "Installed $shell completion to $dest_dir"
            }

            install_completion bash bash
            install_completion elvish elv
            install_completion fish fish
            install_completion powershell ps1
            install_completion zsh zsh

            echo "Finished installing docs."
          '';

          meta = {
            description = "Interception Tools bounce filter with statistics";
            license = pkgs.lib.licenses.mit; # Or Apache-2.0
            maintainers = with pkgs.lib.maintainers; [sinity];
          };
        };
        # You can add other system-specific outputs here, like devShells, apps, etc.
        devShells.default = pkgs.mkShell {
          name = "intercept-bounce-dev";
          buildInputs = with pkgs; [
            rustc
            cargo
            clippy
            rustfmt
            rust-analyzer # Language server

            # System dependencies
            pkg-config
            openssl

            nixpkgs-fmt # For formatting flake.nix
          ];
        };
      }
    ); # Close the eachDefaultSystem call
}
