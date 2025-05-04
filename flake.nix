{
  description = "Debounce plugin for Interception Tools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github.com/numtide/flake-utils"; # Corrected URL
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
        # Get the Rust toolchain
        rust-toolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = ["rust-src"]; # Include rust-src for rust-analyzer
        };
      in {
        # Define packages for this specific system
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "intercept-bounce";
          version = "0.6.0";
          src = ./.;
          # The cargoHash below will need to be updated after these changes.
          # Run `nix build .` and it will tell you the correct hash.
          cargoHash = "sha256-XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX="; # Placeholder - UPDATE THIS HASH

          nativeBuildInputs = [
            pkgs.pkg-config
            # Need cargo/rust for the generation binary during build
            rust-toolchain
            pkgs.stdenv.cc # Might be needed by clap build scripts
          ];
          buildInputs = [ pkgs.openssl ]; # Example: Add openssl if needed
          # If interception tools are needed at build/runtime: pkgs.interception-tools

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
            cp target/release/intercept-bounce $out/bin/
            runHook postInstall

            # Install man page
            echo "Installing man page..."
            mkdir -p $out/share/man/man1
            cp target/generated/intercept-bounce.1 $out/share/man/man1/

            # Install completions
            echo "Installing completions..."
            for shell in bash fish zsh; do # Add other shells if generated
              mkdir -p $out/share/$shell/completions
              cp target/generated/intercept-bounce.$shell $out/share/$shell/completions/
            done
            # Example for nushell (adjust path if needed)
            # mkdir -p $out/share/nushell/completions
            # cp target/generated/intercept-bounce.nu $out/share/nushell/completions/
            # Example for powershell (adjust path if needed)
            # mkdir -p $out/share/powershell/completions
            # cp target/generated/intercept-bounce.ps1 $out/share/powershell/completions/
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
            # Rust toolchain
            rust-toolchain
            cargo # Cargo from toolchain
            clippy # Clippy from toolchain
            rustfmt # Rustfmt from toolchain
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
