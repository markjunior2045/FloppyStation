//! Console prototype entry point. Only wires the library modules together
//! and prints events; all logic lives in the `floppy_station` library.

fn main() {
    println!("floppy-station {}", env!("CARGO_PKG_VERSION"));
}
