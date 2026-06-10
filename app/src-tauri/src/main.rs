// Evita abrir um console extra no Windows em release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    devsplit_lib::run();
}
