{
  pkgs ? import <nixpkgs> { },
}:
pkgs.mkShell rec {
  buildInputs = with pkgs; [
    cargo
    rustc
    rustfmt
    pkg-config
    (writeShellScriptBin "start-recording" ''
      PID=$(pgrep $1)
      if [ -z "$PID" ]; then
        echo "Process $1 not found."
        exit 1
      fi
      sudo perf record -F 999 -p $PID -g
      sudo chown $USER:$(id -gn $USER) perf.data
      perf script -F +pid > /tmp/test.perf
      echo "Recording complete. Firefox-profiler compatible output saved to /tmp/test.perf"
      '')
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
