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
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {inherit system;};
    in {
      packages.default = pkgs.rustPlatform.buildRustPackage {
        pname = "intercept-bounce";
        version = "0.2.0";
        src = ./.;

        # Use cargoHash as cargoSha256 might not work in older nixpkgs versions' rustPlatform.
        # Rely solely on the hash for verifying vendored dependencies.
        # Remove the hash below and run `nix build .# --rebuild` to get the new hash
        # after updating dependencies (like adding phf).
        cargoHash = pkgs.lib.fakeSha256; # Update this after getting the hash mismatch error

        meta = {
          description = "Interception Tools bounce filter with statistics";
          license = pkgs.lib.licenses.mit; # Or Apache-2.0
          maintainers = with pkgs.lib.maintainers; [sinity];
        };
      };
    });
}
