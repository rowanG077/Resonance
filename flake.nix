{
  description = "Resonance development and asset conversion environment";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/34ab99075ac4f7e40cf037eef32cb1c360bb85e9";
  inputs.hvqm4 = {
    url = "github:Tilka/hvqm4/09700757304af4f439f0e50e7a0160cadc4d7a48";
    flake = false;
  };
  outputs =
    {
      self,
      nixpkgs,
      hvqm4,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      eachSystem = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = eachSystem (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          hvqm4 = pkgs.callPackage ./tools/media/hvqm4.nix { hvqm4Src = hvqm4; };
        }
      );
      devShells = eachSystem (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          # nixpkgs limits this derivation's metadata to Linux; upstream KTX
          # also builds its CLI on macOS. Keep that override local and disable
          # documentation/tests in the converter package.
          ktxTools =
            if pkgs.stdenv.hostPlatform.isDarwin then
              pkgs.ktx-tools.overrideAttrs (old: {
                cmakeFlags = [
                  "-DKTX_FEATURE_DOC=OFF"
                  "-DKTX_FEATURE_TESTS=OFF"
                ];
                meta = old.meta // {
                  platforms = [ "aarch64-darwin" ];
                };
              })
            else
              pkgs.ktx-tools;
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
              ffmpeg
              ffmpeg.dev
              vgmstream
              rustPlatform.bindgenHook
              self.packages.${system}.hvqm4
              dolphin-emu
              ktxTools
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
                  pkgs.ffmpeg.lib
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
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.cmake
              pkgs.rustPlatform.bindgenHook
            ];
            buildInputs = [
              pkgs.ffmpeg.dev
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
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
