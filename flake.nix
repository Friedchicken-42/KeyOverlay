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

        # runtimeLibs = with pkgs; [
        #   # libxkbcommon
        #   vulkan-loader
        #   wayland
        # ];

        libs = with pkgs; [
          fontconfig
          freetype
          libxkbcommon
          wayland
          # libx11
          # libxtst
          # libxi
          # libxext
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
      }
    );
}
