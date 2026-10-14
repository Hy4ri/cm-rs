{
  description = "cm-rs: Content Manager for Assetto Corsa, in Rust (GPUI)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in
    {
      packages = forAll (pkgs:
        let
          lib = pkgs.lib;
          cefPlatform = if pkgs.stdenv.hostPlatform.isAarch64 then "linuxarm64" else "linux64";
          cefArchive = {
            type = "minimal";
            name = "cef_binary_151.3.16+gbe1e15d+chromium-151.0.7922.109_${cefPlatform}_minimal.tar.bz2";
            sha1 = if pkgs.stdenv.hostPlatform.isAarch64 then
              "f1a82ce3bde5ea4338da5a41e0bcae22265f36f9" else
              "13042a73aeaf0e853a719d3ddad0751514665bb9";
          };
          # Metadata comes from the official CEF index. Fail on ABI drift rather
          # than letting cef-rs download an unpatched runtime during a build.
          cef-runtime = assert pkgs.cef-binary.version == "151.3.16";
            pkgs.runCommand "cm-rs-cef-runtime-151.3.16" {} ''
              mkdir -p $out
              for entry in ${pkgs.cef-binary}/*; do
                ln -s "$entry" "$out/$(basename "$entry")"
              done
              for entry in ${pkgs.cef-binary}/Release/* ${pkgs.cef-binary}/Resources/*; do
                name=$(basename "$entry")
                # Never put CEF's SwiftShader Vulkan loader on GPUI's library path.
                if [ "$name" != libvulkan.so.1 ]; then
                  ln -s "$entry" "$out/$name"
                fi
              done
              printf '%s' '${builtins.toJSON cefArchive}' > $out/archive.json
            '';

          # Libraries GPUI dlopens at runtime (not linked), so they must be on
          # LD_LIBRARY_PATH for the installed binary.
          runtimeLibs = with pkgs; [
            vulkan-loader
            wayland
            libxkbcommon
            libx11
            libxcb
            libxkbfile
            fontconfig
            freetype
          ];

          cm-rs = pkgs.rustPlatform.buildRustPackage {
            pname = "cm-rs";
            version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;

            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./crates
                ./packaging
              ];
            };

            cargoLock.lockFile = ./Cargo.lock;
            CEF_PATH = "${cef-runtime}";
            cargoBuildFlags = [ "-p" "cm-app" ];
            doCheck = false;

            nativeBuildInputs = with pkgs; [
              pkg-config
              cmake
              ninja
              makeWrapper
              copyDesktopItems
              installShellFiles
            ];

            buildInputs = with pkgs; [
              fontconfig
              freetype
              libxkbcommon
              wayland
              libxcb
              libx11
              vulkan-loader
              openssl
              zstd
            ];

            desktopItems = [
              (pkgs.makeDesktopItem {
                name = "cm-rs";
                desktopName = "Content Manager (Rust)";
                genericName = "Assetto Corsa Launcher";
                comment = "Native launcher for Assetto Corsa: Quick Drive, cars, tracks, mod install";
                exec = "cm-rs";
                icon = "cm-rs";
                terminal = false;
                categories = [ "Game" ];
                keywords = [ "Assetto Corsa" "AC" "Racing" "Content Manager" "Proton" ];
                startupWMClass = "cm-rs";
              })
            ];

            postInstall = ''
              install -Dm644 packaging/cm-rs.svg $out/share/icons/hicolor/scalable/apps/cm-rs.svg
            '';

            postFixup = ''
              wrapProgram $out/bin/cm-rs \
                --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}:${cef-runtime} \
                --set CM_RS_CEF_PATH ${cef-runtime} \
                --prefix PATH : ${lib.makeBinPath [ pkgs.xdg-utils ]}
            '';

            meta = {
              description = "Content Manager for Assetto Corsa, rewritten in Rust with GPUI";
              license = lib.licenses.mspl;
              mainProgram = "cm-rs";
              platforms = systems;
            };
          };
        in
        {
          inherit cm-rs cef-runtime;
          default = cm-rs;
        });

      apps = forAll (pkgs: rec {
        cm-rs = {
          type = "app";
          program = "${self.packages.${pkgs.system}.cm-rs}/bin/cm-rs";
        };
        default = cm-rs;
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.system}.cm-rs ];
          packages = with pkgs; [ cargo rustc clippy rustfmt rust-analyzer ];
          # Same dlopen'd libraries as the wrapped binary, so `cargo run` works.
          LD_LIBRARY_PATH = (pkgs.lib.makeLibraryPath (with pkgs; [
            vulkan-loader wayland libxkbcommon libx11 libxcb fontconfig freetype
          ])) + ":${self.packages.${pkgs.system}.cef-runtime}";
          CEF_PATH = "${self.packages.${pkgs.system}.cef-runtime}";
          CM_RS_CEF_PATH = "${self.packages.${pkgs.system}.cef-runtime}";
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      });

      overlays.default = final: _prev: {
        cm-rs = self.packages.${final.system}.cm-rs;
      };

      formatter = forAll (pkgs: pkgs.nixpkgs-fmt);
    };
}
