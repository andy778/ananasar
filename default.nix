# `nix-build` without flakes: the full program from the system's nixpkgs.
{
  pkgs ? import <nixpkgs> { },
}:
pkgs.callPackage ./nix/package.nix { }
