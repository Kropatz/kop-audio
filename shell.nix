{
  pkgs ? import <nixpkgs> { },
}:
pkgs.mkShell rec {
  buildInputs = with pkgs; [
    cargo
    rustc
    rustfmt
    pkg-config
  ];

  nativeBuildInputs = with pkgs; [
    libopus
    libpulseaudio
  ];

  shellHook =
    let
      libraries = with pkgs; [
      ];
    in
    ''
      export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath libraries}:$LD_LIBRARY_PATH
    '';
}
