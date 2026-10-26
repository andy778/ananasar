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
      packages = forAllSystems (
        pkgs:
        let
          # The whole program: CLI, `ananasar gui` and `ananasar mcp`.
          # The GUI needs Rust 1.95, so this wants a recent nixpkgs.
          ananasar = pkgs.callPackage ./nix/package.nix { };
        in
        {
          inherit ananasar;
          # The small headless CLI, `--no-default-features` (no egui, no MCP).
          ananasar-cli = ananasar.override {
            withGui = false;
            withMcp = false;
          };
          default = ananasar;
        }
      );

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.ananasar}/bin/ananasar";
          meta.description = "Modbus/TCP client, server and storm tool";
        };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.ananasar ];
          packages = [
            pkgs.clippy
            pkgs.rustfmt
          ];
          LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
            pkgs.lib.makeLibraryPath self.packages.${pkgs.stdenv.hostPlatform.system}.ananasar.passthru.guiLibs
          );
        };
      });
    };
}
