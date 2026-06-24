//! Rebuild when locale YAML files change.
//!
//! `rust_i18n::i18n!` embeds translations at compile time. Without explicit
//! `rerun-if-changed` hints Cargo incremental builds may skip recompilation
//! when only `locales/*.yml` files are edited.

fn main() {
    let locales = std::path::Path::new("locales");
    println!("cargo:rerun-if-changed=locales");
    if locales.is_dir() {
        emit_rerun_if_changed(locales);
    }
}

fn emit_rerun_if_changed(dir: &std::path::Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            emit_rerun_if_changed(&path);
        }
    }
}
