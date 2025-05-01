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
        version = "0.6.0"; # Updated version

        src = ./.;
        cargoHash = "sha256-t88QzISCYdgSum6nngQz42N52u9B/0zrz/+vlb849fw="; # Updated hash for 544425e

        meta = {
          description = "Interception Tools bounce filter with statistics";
          license = pkgs.lib.licenses.mit; # Or Apache-2.0
          maintainers = with pkgs.lib.maintainers; [sinity];
        };
      };
    });
}
