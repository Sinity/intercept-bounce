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
        version = "0.1.1";
        src = ./.;
        cargoHash = "sha256-cO7yquZR5cN95am47vX7CYpMdQY1DDxHEsYdwRgWiW4=";

        meta = {
          description = "Interception Tools bounce filter";
          license = pkgs.lib.licenses.mit; # Or Apache-2.0
          maintainers = with pkgs.lib.maintainers; [sinity];
        };
      };
    });
}
