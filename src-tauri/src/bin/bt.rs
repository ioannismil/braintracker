//! `bt`: BrainTracker's command line as a console program, so terminals wait for it and show
//! its output (the main `braintracker.exe` is a windowed program on Windows). See `cli.rs`.

fn main() {
    std::process::exit(braintracker_lib::console_main());
}
