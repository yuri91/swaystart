{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/release-24.05";

    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

    naersk.url = "github:nmattia/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay, naersk, ... } @ inputs:
  let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      overlays = [ rust-overlay.overlays.default ];
    };
    rust-build = pkgs.rust-bin.stable.latest.default.override {
      extensions = [ "rust-src" "rust-analyzer" ];
      targets = [];
    };
    naersk-lib = naersk.lib.${system}.override {
      rustc = rust-build;
      cargo = rust-build;
    };
    swaystart = naersk-lib.buildPackage {
      pname = "swaystart";
      root = ./.;
      buildInputs = with pkgs; [
        glib
        libxkbcommon
        wayland
      ];
      nativeBuildInputs = with pkgs; [
        pkg-config
        rust-build
      ];
    };
  in
  {
    devShell.${system} = pkgs.mkShell {
      packages = with pkgs; [
        git
        cargo-edit
      ];
      inputsFrom = with pkgs; [
        swaystart
      ];
      RUST_SRC_PATH = "${rust-build}/lib/rustlib/src/rust/library";
    };
    packages.${system}.default = swaystart;
  };
}
