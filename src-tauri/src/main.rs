// Ẩn cửa sổ console trên Windows ở bản release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mpm_lib::run();
}
