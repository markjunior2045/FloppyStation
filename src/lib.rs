//! Floppy Station core library.
//!
//! Holds all the logic (floppy detection, disk config, game launching and
//! local settings) so it can be reused unchanged by the console prototype
//! and, later, by the Tauri app.

pub mod floppy;
pub mod settings;
