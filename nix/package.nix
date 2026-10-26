{
  lib,
  stdenv,
  rustPlatform,
  makeWrapper,
  pkg-config,
  libGL,
  libxkbcommon,
  vulkan-loader,
  wayland,
  libx11,
  libxcursor,
  libxi,
  libxrandr,
  withGui ? true,
  withMcp ? true,
}:

let
  # What winit/wgpu load at run time on Linux.
  guiLibs = lib.optionals stdenv.hostPlatform.isLinux [
    libGL
    libxkbcommon
    vulkan-loader
    wayland
    libx11
    libxcursor
    libxi
    libxrandr
  ];
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = if withGui then "ananasar" else "ananasar-cli";
  version = (lib.importTOML ../Cargo.toml).workspace.package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../clippy.toml
      ../crates
      ../assets
      ../docs
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = [
    "--package"
    "ananasar-cli"
  ];
  buildNoDefaultFeatures = true;
  buildFeatures = lib.optional withGui "gui" ++ lib.optional withMcp "mcp";

  nativeBuildInputs = [ pkg-config ] ++ lib.optional withGui makeWrapper;

  # The GUI tests need a display and a GPU.
  doCheck = !withGui;

  postFixup = lib.optionalString (withGui && stdenv.hostPlatform.isLinux) ''
    wrapProgram $out/bin/ananasar \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath guiLibs}
  '';

  passthru = { inherit guiLibs; };

  meta = {
    description = "Modbus/TCP client, server and storm tool, a Rust port of Ananas";
    homepage = "https://github.com/andy778/ananasar";
    license = lib.licenses.mit;
    mainProgram = "ananasar";
    platforms = lib.platforms.unix;
  };
})
