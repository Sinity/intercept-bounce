{
  description = "Debounce plugin for Interception Tools";

 inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-24.05";
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
          # You will need to run `nix build .` once, copy the hash from the error,
          # and paste it here.
          cargoSha256 = "YOUR_HASH_HERE"; # Make sure this matches the hash from the nix build error

         meta = {
            description = "Interception Tools debounce filter";
            license     = pkgs.lib.licenses.mit;
            maintainers = [ "sinity" ]; # Replace with your GitHub username
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
