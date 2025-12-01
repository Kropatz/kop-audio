To build:
1. Run `nix-shell` to get all dependencies
2. `cargo run`

Alternatively install needed dependencies using your distros package manager (listed in shell.nix).

If building on NixOS, to make the built binary run on on non-nix systems you have to patch the interpreter like this: `patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2 ./target/release/kop-audio`
