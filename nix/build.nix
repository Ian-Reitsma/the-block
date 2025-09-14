{ pkgs ? import <nixpkgs> {} }:
pkgs.rustPlatform.buildRustPackage {
  pname = "the-block";
  version = "0.1";
  src = ./.;
  cargoLock = { lockFile = ../Cargo.lock; }; 
}
