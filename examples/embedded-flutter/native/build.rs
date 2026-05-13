// On macOS and iOS, the transitive dep `whoami` (via sqlx-postgres in
// the cratestack facade) references SystemConfiguration.framework's
// `SCDynamicStoreCopyComputerName`. Cargo doesn't link Apple frameworks
// automatically — the consumer crate has to declare them. We emit the
// link line at build time so the dylib resolves at link time on Darwin
// targets without surfacing the framework name in every dependent's
// own build script.
//
// On Linux / Windows / Android there's nothing to do here.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let is_apple = target.contains("-apple-");
    if is_apple {
        println!("cargo:rustc-link-lib=framework=SystemConfiguration");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }
}
