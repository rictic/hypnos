{
  description = "A Nix flake for the hypnos project";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rustVersion = pkgs.rust-bin.stable."1.75.0".default; # Adjust version as needed
      in
      with pkgs;
      {
        packages = rec {
          hypnos = pkgs.rustPlatform.buildRustPackage {
            pname = "hypnos";
            version = "1.2.1"; # Should match Cargo.toml

            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            nativeBuildInputs = [
              pkg-config
            ];

            buildInputs = [
              openssl
              # Add other system dependencies if needed by your Rust crates
            ];

            # If your tests require network access or other special conditions,
            # you might need to configure them here.
            # By default, `cargo test` is run.
          };
          default = hypnos;
        };

        checks = {
          # Run tests
          hypnos-tests = self.packages.${system}.hypnos.overrideAttrs (oldAttrs: {
            # The buildRustPackage already runs `cargo test` by default
            # if you need to customize it, you can do it here.
            # For example, to pass specific arguments to cargo test:
            # buildPhase = ''
            #   runHook preBuild
            #   cargo test --verbose -- --skip some_integration_test
            #   runHook postBuild
            # '';
            # Or to ensure tests are run in a specific environment:
            # checkPhase = ''
            #  export SOME_ENV_VAR="value"
            #  cargo test --verbose
            # '';
          });
          default = self.checks.${system}.hypnos-tests;
        };

        devShells.default = mkShell {
          useLLVM = true;
          packages = [
            rustVersion
            cargo
            rustfmt
            clippy
            rust-analyzer
            openssl
            pkg-config
            # Add other development tools here
          ];
          # Environment variables for the shell
          RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
        };
      }
    );
}
