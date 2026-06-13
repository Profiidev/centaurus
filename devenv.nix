{ pkgs, ... }:

{
  packages = with pkgs; [
    pkg-config
    openssl
  ];
}
