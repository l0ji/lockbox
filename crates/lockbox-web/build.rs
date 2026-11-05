//! Embed `static/` into the binary so it ships as a single executable.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=static");

    let manifest = env::var("CARGO_MANIFEST_DIR").expect("manifest dir");
    let root = Path::new(&manifest).join("static");
    let out = Path::new(&env::var("OUT_DIR").expect("out dir")).join("static.rs");

    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();

    let mut source = String::from(
        "/// Generated code for all files under `static/`.\n\
         pub const ASSETS: &[(&str, &[u8])] = &[\n",
    );
    for file in files {
        let path = file.strip_prefix(&root).expect("file is under static");
        let key = path.to_string_lossy().replace('\\', "/");
        let source_path = file.to_string_lossy().replace('\\', "/");
        source.push_str(&format!(
            "    ({key:?}, include_bytes!({source_path:?})),\n"
        ));
    }
    source.push_str("];\n");

    fs::write(out, source).expect("write embedded assets");
}

/// Walks the `dir` and grabs all the files that it needs.
fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => panic!("cannot read {}: {err}", dir.display()),
    };

    for entry in entries {
        let path = entry.expect("read dir entry").path();
        let meta = fs::symlink_metadata(&path).expect("stat static entry");
        if meta.is_dir() {
            collect(&path, files);
        } else if meta.is_file() {
            files.push(path);
        }
    }
}
