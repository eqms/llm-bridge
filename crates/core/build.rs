// Zielplattform für den Namen der CLI-Programmdatei im Release (`update::cli_asset`).
fn main() {
    println!("cargo:rustc-env=LLM_BRIDGE_TARGET={}", std::env::var("TARGET").expect("TARGET"));
}
