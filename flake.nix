{
  description = "Ananasar: Rust port of the Ananas Modbus/TCP tool";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        # The whole program: CLI, `ananasar gui` and `ananasar mcp`.
        # The GUI needs Rust 1.95, so this wants a recent nixpkgs.
        ananasar = pkgs.callPackage ./nix/package.nix { };
        # The small headless CLI (no egui, nothing to link on a server).
        ananasar-cli = ananasar.override { withGui = false; };
        default = ananasar;
      });

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.system}.ananasar}/bin/ananasar";
        };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.system}.ananasar ];
          packages = [
            pkgs.clippy
            pkgs.rustfmt
          ];
          LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
            pkgs.lib.makeLibraryPath (self.packages.${pkgs.system}.ananasar.guiLibs or [ ])
          );
        };
      });
    };
}
