fn main() {
    // Frontend work package: winit window + softbuffer presentation + cpal
    // audio + ROM loading. Until then, prove the core links.
    eprintln!(
        "slopgb {}: frontend not yet implemented ({}x{} target)",
        env!("CARGO_PKG_VERSION"),
        slopgb_core::SCREEN_W,
        slopgb_core::SCREEN_H,
    );
    std::process::exit(2);
}
