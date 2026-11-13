{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      fenix,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        toolchain = fenix.packages.${system}.latest.withComponents [
          "cargo"
          "clippy"
          "rust-src"
          "rustc"
          "rustfmt"
        ];

        libs = with pkgs; [
          fontconfig
          freetype
          libxkbcommon
          wayland
          libxcb
          vulkan-loader
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          packages = [
            toolchain
            pkgs.pkg-config
          ]
          ++ libs;

          shellHook = ''
            export LD_LIBRARY_PATH="${nixpkgs.lib.makeLibraryPath libs}";
          '';
        };

        packages.default =
          (pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          }).buildRustPackage
            {
              pname = "KeyOverlay";
              version = "0.1.0";
              src = self;
              cargoLock.lockFile = ./Cargo.lock;

              nativeBuildInputs = with pkgs; [ pkg-config makeWrapper ];
              buildInputs = libs;

              postInstall = ''
                wrapProgram $out/bin/KeyOverlay \
                  --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath libs}"
              '';
            };
      }
    );
}
