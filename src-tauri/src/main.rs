// Without this the binary links against the Windows console subsystem, so
// launching the app opens a black terminal window behind it that has to be
// closed separately. Release builds are GUI-only; debug builds keep the console
// so `println!` from the test harness and tooling stays visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    octopus_chat_lib::run();
}
