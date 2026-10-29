//! Floppy Station core library.
//!
//! Holds all the logic (floppy detection, disk config, game launching and
//! local settings), shared by the Tauri app and the console tool.

pub mod controller;
pub mod diskconfig;
pub mod floppy;
pub mod kinds;
pub mod launcher;
pub mod settings;
pub mod startup;
pub mod trust;

mod win32;
