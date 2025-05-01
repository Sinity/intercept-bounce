{
  description = "Debounce plugin for Interception Tools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github.com/numtide/flake-utils"; # Corrected flake-utils URL
  };

  outputs = {
    nixpkgs,
    flake-utils,
    ...
  : let
      pkgs = import nixpkgs {inherit system;};
    in {
      packages.default = pkgs.rustPlatform.buildRustPackage {
        pname = "intercept-bounce";
        version = "0.6.0"; # Updated version

        src = ./.;
        # The cargoHash below will need to be updated after these changes.
        # Run `nix build .` and it will tell you the correct hash.
        cargoHash = "sha256-t88QzISCYdgSum6nngQz42N52u9B/0zrz/+vlb849fw="; # Placeholder - UPDATE THIS HASH

        meta = {
          description = "Interception Tools bounce filter with statistics";
          license = pkgs.lib.licenses.mit; # Or Apache-2.0
          maintainers = with pkgs.lib.maintainers; [sinity];
        };
      };
    });
}
