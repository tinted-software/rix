{
  inputs = {
    nixpkgs.url = "github:tinted-software/nixpkgs";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "riscv64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
          };
        in
        with pkgs;
        {
          default = rustPlatform.buildRustPackage {
            pname = "rix";
            version = "0.1.0";
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
          };
        }
      );
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            name = "rix-devshell";
            packages = with pkgs; [
              rustc
              cargo
              clippy
              rustfmt
              cargo-nextest
            ];
            shellHook = ''
              echo "theos dev shell"
              echo "  nix run .#<cmd>"
              echo "  nix build"
              echo "  nix develop"
            '';
          };
        }
      );
    };
}
