{
  description = "Niri Panel - A GNOME Panel clone for Wayland/Niri";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            pkg-config
            
            # Wayland dependencies
            wayland
            wayland-protocols
            wayland-scanner
            libxkbcommon
            
            # GTK4 and related
            gtk4
            gtk4-layer-shell
            glib
            cairo
            pango
            gdk-pixbuf
            
            # Icon themes
            adwaita-icon-theme
            hicolor-icon-theme
            
            # Search tools (optional runtime deps)
            fd
            ripgrep
            fzf
            
            # Additional tools
            cargo-watch
            rustfmt
            clippy
            
            # Include our own packages in the development environment
            self.packages.${system}.default
            self.packages.${system}.niri-panel-ctrl
          ];

          shellHook = ''
            echo "Niri Panel development environment"
            echo "Run 'cargo build' to build the panel"
            echo "Run 'cargo run' to start the panel"
            echo "Run 'niri-panel-ctrl list' to see available widgets"
            
            # Set up icon theme paths
            export XDG_DATA_DIRS="${pkgs.adwaita-icon-theme}/share:${pkgs.hicolor-icon-theme}/share:$XDG_DATA_DIRS"
          '';

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
            pkgs.wayland
            pkgs.gtk4
            pkgs.cairo
            pkgs.pango
            pkgs.glib
            pkgs.libxkbcommon
          ];
        };

        packages = {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "niri-panel";
            version = "0.1.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
            };
            
            nativeBuildInputs = with pkgs; [
              pkg-config
              wrapGAppsHook
            ];
            
            buildInputs = with pkgs; [
              wayland
              gtk4
              gtk4-layer-shell
              glib
              cairo
              pango
              libxkbcommon
            ];
          };
          
          niri-panel-ctrl = pkgs.rustPlatform.buildRustPackage {
            pname = "niri-panel-ctrl";
            version = "0.1.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
            };
            
            nativeBuildInputs = with pkgs; [
              pkg-config
            ];
            
            buildInputs = with pkgs; [
              wayland
              libxkbcommon
            ];
            
            # Only build and install the niri-panel-ctrl binary
            buildPhase = ''
              runHook preBuild
              cargo build --release --bin niri-panel-ctrl
              runHook postBuild
            '';
            
            installPhase = ''
              runHook preInstall
              mkdir -p $out/bin
              cp target/release/niri-panel-ctrl $out/bin/
              runHook postInstall
            '';
          };
        };
      });
}
