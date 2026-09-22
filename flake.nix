{
  description = "Resonance development and asset conversion environment";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/34ab99075ac4f7e40cf037eef32cb1c360bb85e9";
  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      eachSystem = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = eachSystem (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          platformLibraries =
            with pkgs;
            lib.optionals stdenv.hostPlatform.isLinux [
              alsa-lib
              dbus
              udev
              vulkan-loader
              libx11
              libxcursor
              libxi
              libxrandr
              libxkbcommon
              wayland
            ];
          common =
            with pkgs;
            [
              rustc
              cargo
              rustfmt
              clippy
              rust-analyzer
              rustPlatform.rustLibSrc
              clang
              cmake
              ninja
              pkg-config
              git
              ripgrep
              jq
              (python3.withPackages (p: [
                p.numpy
                p.scipy
                p.pillow
              ]))
              lz4
              dolphin-emu
              nodejs
              typescript
              esbuild
              cargo-nextest
              lldb
              nixfmt
            ]
            ++ platformLibraries
            ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
              pkgs.xorg-server
              pkgs.xdotool
            ];
          shell =
            extras:
            pkgs.mkShell {
              packages = common ++ extras;
              RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}/lib/rustlib/src/rust/library";
              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (
                platformLibraries
                ++ [
                  pkgs.lz4
                ]
              );
            };
        in
        {
          default = shell [ ];
          art = shell [ pkgs.blender ];
        }
      );
      formatter = eachSystem (system: nixpkgs.legacyPackages.${system}.nixfmt);
      checks = eachSystem (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          source = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./LICENSE
              ./rust-toolchain.toml
              ./apps
              ./crates
              ./scripts
              ./tools/oracle
            ];
          };
        in
        {
          format =
            pkgs.runCommand "resonance-format"
              {
                nativeBuildInputs = [
                  pkgs.cargo
                  pkgs.rustfmt
                ];
              }
              ''
                cd ${source}
                cargo fmt --all --check
                touch "$out"
              '';
          workspace = pkgs.rustPlatform.buildRustPackage {
            pname = "resonance-workspace-check";
            version = "0.1.0";
            src = source;
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes."ffv1-0.0.0" = "sha256-/ENsKVXIivMPsubqzfKUdGE/4rz8jev3LO/L+/4EVjY=";
            };
            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.cmake
            ];
            buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
              pkgs.alsa-lib
              pkgs.dbus
              pkgs.udev
              pkgs.vulkan-loader
              pkgs.libx11
              pkgs.libxcursor
              pkgs.libxi
              pkgs.libxrandr
              pkgs.libxkbcommon
              pkgs.wayland
            ];
            cargoBuildFlags = [ "--workspace" ];
            cargoTestFlags = [ "--workspace" ];
            doCheck = true;
          };
        }
      );
    };
}
