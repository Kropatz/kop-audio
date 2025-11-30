fn main() {
    pkg_config::probe_library("libpulse-simple").unwrap();
    pkg_config::probe_library("opus").unwrap();
}
