{
  description = "eframe devShell";

  # Cross-compilation shamelessly lifted from https://mediocregopher.com/posts/x-compiling-rust-with-nix

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
    flake-utils.url = "github:numtide/flake-utils";

    # Cross-compile
    fenix.url = "github:nix-community/fenix";
    naersk.url = "github:nix-community/naersk/master";

    # Shell
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
  };

  outputs = { self, nixpkgs, flake-utils, fenix, naersk, rust-overlay, ... }:
  let
      buildTargets = {
        "x86_64-linux" = {
          crossSystemConfig = "x86_64-unknown-linux-musl";
          rustTarget = "x86_64-unknown-linux-musl";
        };

        "i686-linux" = {
          crossSystemConfig = "i686-unknown-linux-musl";
          rustTarget = "i686-unknown-linux-musl";
        };

        "aarch64-linux" = {
          crossSystemConfig = "aarch64-unknown-linux-musl";
          rustTarget = "aarch64-unknown-linux-musl";
        };

        # Old Raspberry Pi's
        "armv6l-linux" = {
          crossSystemConfig = "armv6l-unknown-linux-musleabihf";
          rustTarget = "arm-unknown-linux-musleabihf";
        };

        "x86_64-windows" = {
          crossSystemConfig = "x86_64-w64-mingw32";
          rustTarget = "x86_64-pc-windows-gnu";
          makeBuildPackageAttrs = pkgsCross: {
            depsBuildBuild = [
              pkgsCross.stdenv.cc
              pkgsCross.windows.pthreads
            ];
          };
        };
      };

      # eachSystem [system] (system: ...)
      #
      # Returns an attrset with a key for every system in the given array, with
      # the key's value being the result of calling the callback with that key.
      eachSystem = supportedSystems: callback: builtins.foldl'
        (overall: system: overall // { ${system} = callback system; })
        {}
        supportedSystems;

      # eachCrossSystem [system] (buildSystem: targetSystem: ...)
      #
      # Returns an attrset with a key "$buildSystem.cross-$targetSystem" for
      # every combination of the elements of the array of system strings. The
      # value of the attrs will be the result of calling the callback with each
      # combination.
      #
      # There will also be keys "$system.default", which are aliases of
      # "$system.cross-$system" for every system.
      #
      eachCrossSystem = supportedSystems: callback:
        eachSystem supportedSystems (buildSystem: builtins.foldl'
            (inner: targetSystem: inner // {
              "cross-${targetSystem}" = callback buildSystem targetSystem;
            })
            { default = callback buildSystem buildSystem; }
            supportedSystems
        );

      mkPkgs = buildSystem: targetSystem: import nixpkgs ({
        system = buildSystem;
      } // (if targetSystem == null then {} else {
        # The nixpkgs cache doesn't have any packages where cross-compiling has
        # been enabled, even if the target platform is actually the same as the
        # build platform (and therefore it's not really cross-compiling). So we
        # only set up the cross-compiling config if the target platform is
        # different.
        crossSystem.config = buildTargets.${targetSystem}.crossSystemConfig;
      }));

    in {
      packages = eachCrossSystem
        (builtins.attrNames buildTargets)
        (buildSystem: targetSystem: let
           pkgs = mkPkgs buildSystem null;
          pkgsCross = mkPkgs buildSystem targetSystem;
          rustTarget = buildTargets.${targetSystem}.rustTarget;

          fenixPkgs = fenix.packages.${buildSystem};

          mkToolchain = fenixPkgs: fenixPkgs.toolchainOf {
            channel = "1.83";
            sha256 = "sha256-s1RPtyvDGJaX/BisLT+ifVfuhDT1nZkZ1NcK8sbwELM=";
          };

          toolchain = fenixPkgs.combine [
            (mkToolchain fenixPkgs).rustc
            (mkToolchain fenixPkgs).cargo
            (mkToolchain fenixPkgs.targets.${rustTarget}).rust-std
          ];

          buildPackageAttrs = if
            builtins.hasAttr "makeBuildPackageAttrs" buildTargets.${targetSystem}
          then
            buildTargets.${targetSystem}.makeBuildPackageAttrs pkgsCross
          else
            {};

          naersk-lib = pkgs.callPackage naersk {
            cargo = toolchain;
            rustc = toolchain;
          };
        in
          naersk-lib.buildPackage (buildPackageAttrs // rec {
            src = ./.;
            strictDeps = true;
            doCheck = false;
            cargoBuildOptions = x: x ++ [ "-p" "planchette" "-p" "seance-app" ];
            cargoTestOptions = x: x ++ [ "-p" "planchette" "-p" "seance-app" ];

            TARGET_CC = "${pkgsCross.stdenv.cc}/bin/${pkgsCross.stdenv.cc.targetPrefix}cc";

            CARGO_BUILD_TARGET = rustTarget;
            CARGO_BUILD_RUSTFLAGS = [
              "-C" "target-feature=+crt-static"

              # https://github.com/rust-lang/cargo/issues/4133
              "-C" "linker=${TARGET_CC}"
            ];
          })
        );
    } //
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        nativeBuildInputs = with pkgs; [
          rustToolchain
          cargo-deb
          typos

          dpkg
        ];
        buildInputs = with pkgs; [
          pkg-config

          openssl.dev

          # So many things required for wgpu
          libxkbcommon
          libGL
          fontconfig
          wayland
          xorg.libXcursor
          xorg.libXrandr
          xorg.libXi
          xorg.libX11
          alsa-lib
          freetype
          shaderc
          directx-shader-compiler
          cmake
          vulkan-headers
          vulkan-loader
          vulkan-tools
          vulkan-tools-lunarg
          vulkan-extension-layer
          vulkan-validation-layers
        ];
      in with pkgs; {
        formatter = pkgs.alejandra;

        devShells.default = mkShell rec {
          inherit buildInputs nativeBuildInputs;

          LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${builtins.toString (pkgs.lib.makeLibraryPath buildInputs)}";
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      });
}
