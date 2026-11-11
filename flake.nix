{
  description = "Reproducible project-local and global agent skill manager";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      mkPackage =
        pkgs:
        pkgs.rustPlatform.buildRustPackage {
          pname = "skillctl";
          inherit ((lib.importTOML ./Cargo.toml).package) version;

          src = ./.;
          # Dependencies come from Cargo.lock, so there is no vendor hash to maintain.
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          nativeCheckInputs = [ pkgs.git ];

          postFixup = ''
            wrapProgram "$out/bin/skillctl" --prefix PATH : ${lib.makeBinPath [ pkgs.git ]}
          '';

          meta = {
            description = "Reproducible project-local and global agent skill manager";
            homepage = "https://github.com/zydtiger/skillctl";
            license = lib.licenses.mit;
            platforms = lib.platforms.unix;
            mainProgram = "skillctl";
          };
        };
    in
    {
      packages = lib.genAttrs systems (
        system:
        let
          skillctl = mkPackage nixpkgs.legacyPackages.${system};
        in
        {
          inherit skillctl;
          default = skillctl;
        }
      );
    };
}
