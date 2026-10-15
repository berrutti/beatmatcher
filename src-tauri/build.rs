use std::fmt::Write;

/// Every `.json` in `mappings/`, so contributing a mapping is a JSON file and not
/// also a Rust edit.
fn mapping_files() -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("mappings");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();

    let mut source = format!(
        "pub(super) const MAPPING_FILES: [&str; {}] = [\n",
        paths.len()
    );
    for path in &paths {
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(source, "    include_str!({path:?}),").expect("a string");
    }
    source.push_str("];\n");
    source
}

fn main() {
    let out =
        std::path::Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("mapping_files.rs");
    std::fs::write(&out, mapping_files())
        .unwrap_or_else(|error| panic!("{}: {error}", out.display()));
    tauri_build::build()
}
