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
          cargoHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="; # Placeholder - UPDATE THIS HASH

          nativeBuildInputs = [ pkgs.pkg-config ]; # Add pkg-config if needed by dependencies
          buildInputs = [ pkgs.openssl ]; # Example: Add openssl if needed
          # If interception tools are needed at build/runtime: pkgs.interception-tools
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
