// Release builds are GUI apps with no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    translator_lib::run()
}
