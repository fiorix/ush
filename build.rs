fn main() {
    let packaged = std::env::var("USH_PACKAGED").unwrap_or_else(|_| "source".into());
    println!("cargo:rustc-env=USH_PACKAGED={packaged}");
}
