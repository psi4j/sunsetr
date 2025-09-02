{
  description = "sunsetr: Automatic blue light filter for Wayland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      supportedSystems = [ "x86_64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;

      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

      makePackage =
        system:
        let
          pkgs = pkgsFor system;
          rustToolchain = pkgs.rust-bin.stable.latest.default;
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = "sunsetr";
          version =
            let
              pkgVer = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
              commit = self.shortRev or "dev";
            in
            "${pkgVer}-${commit}";

          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./src
              ./tests
              ./Cargo.toml
              ./Cargo.lock
              ./README.md
            ];
          };

          cargoLock = {
            lockFile = ./Cargo.lock;
            allowBuiltinFetchGit = true;
          };

          nativeBuildInputs = [
            rustToolchain
            pkgs.pkg-config
          ];
          buildInputs = [ ];
          doCheck = true;
        };
    in
    {
      packages = forAllSystems (system: {
        sunsetr = makePackage system;
        default = makePackage system;
      });

      checks = forAllSystems (system: {
        default = makePackage system;
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          rustToolchain = pkgs.rust-bin.stable.latest.default;
        in
        {
          default = pkgs.mkShell {
            packages = [
              rustToolchain
              pkgs.cargo-nextest
              pkgs.cargo-edit
              pkgs.rustfmt
              pkgs.clippy
            ];
            nativeBuildInputs = [ pkgs.pkg-config ];
          };
        }
      );
    };
}
