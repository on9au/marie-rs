{
  description = "MARIE assembler, VM, linter and language server";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = {
    self,
    nixpkgs,
  }: let
    systems = ["aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux"];
    forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
  in {
    packages = forAllSystems (pkgs: {
      default = pkgs.rustPlatform.buildRustPackage {
        pname = "marie-rs";
        version = "0.1.0";
        src = self;
        cargoLock.lockFile = ./Cargo.lock;
        # The CLI tests signal the child with `kill`.
        nativeCheckInputs = [pkgs.procps];
        meta.mainProgram = "marie";
      };
    });

    devShells = forAllSystems (pkgs: {
      default = pkgs.mkShell {
        packages = with pkgs; [
          cargo
          clippy
          python3 # scripts/lsp-smoke.py
          rust-analyzer
          rustc
          rustfmt
        ];
        RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
      };
    });
  };
}
