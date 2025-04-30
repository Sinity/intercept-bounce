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
          version      = "0.1.0";
          src          = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # You will need to run `nix build .#` once after `cargo build` (or after updating dependencies),
          # copy the hash from the nix build error message, and paste it here.
          cargoSha256 = "YOUR_HASH_HERE";

         meta = {
            description = "Interception Tools debounce filter";
            license     = pkgs.lib.licenses.mit; # Or Apache-2.0
            maintainers = with pkgs.lib.maintainers; [ sinity ]; # Using your GitHub username from Cargo.toml authors
          };
        };

       # simple module so users can `programs.intercept-bounce.enable = true`
        nixosModules.intercept-bounce = { config, pkgs, lib, ... }: {
          options.programs.intercept-bounce.enable = lib.mkEnableOption "intercept-bounce";
          config = lib.mkIf config.programs.intercept-bounce.enable {
            environment.systemPackages = [ self.packages.${system}.default ];
          };
        };
      });
}
