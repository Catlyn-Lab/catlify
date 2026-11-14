fn main() {
    // The static FFmpeg build uses zlib (PNG cover art, compressed MOV atoms); every
    // target we link it on (macOS, iOS, Android) ships libz in the system SDK.
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(os.as_str(), "macos" | "ios" | "android") {
        println!("cargo:rustc-link-lib=z");
    }
    tauri_build::build()
}
