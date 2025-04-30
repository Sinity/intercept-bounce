{
  description = "Debounce plugin for Interception Tools";

 inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable"; # Changed from nixos-24.05 to support newer Cargo.lock format
    flake-utils.url  = "github:numtide/flake-utils";
  };

 outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let pkgs = import nixpkgs { inherit system; };
      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname        = "intercept-bounce";
          version      = "0.1.1"; # Updated version
          src          = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # This hash verifies the vendored dependencies based on Cargo.lock.
          # Update this hash using the output of `nix build .#` if Cargo.lock changes.
          cargoSha256 = "YOUR_HASH_HERE"; # TODO: Replace with actual hash after `nix build`

         meta = {
            description = "Interception Tools bounce filter";
            license     = pkgs.lib.licenses.mit; # Or Apache-2.0
            maintainers = with pkgs.lib.maintainers; [ sinity ];
          };
        };

        # Removed NixOS module - users should reference the package directly
        # in their udevmon configuration. See examples/udevmon.yaml.
      });
}
