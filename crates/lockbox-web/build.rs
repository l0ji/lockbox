//! Embed `static/` into the binary so it ships as a single executable.

use std::{
    env, error, fs,
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn error::Error>> {
    println!("cargo:rerun-if-changed=static");

    let manifest = env::var("CARGO_MANIFEST_DIR")?;
    let root = Path::new(&manifest).join("static");
    let out = Path::new(&env::var("OUT_DIR")?).join("static.rs");

    let mut files = Vec::new();
    collect(&root, &mut files)?;
    files.sort();

    let mut source = String::from(
        "/// Generated code for all files under `static/`.\n\
         pub const ASSETS: &[(&str, &[u8])] = &[\n",
    );
    for file in files {
        let path = file
            .strip_prefix(&root)
            .map_err(|_| format!("{} is outside {}", file.display(), root.display()))?;
        let key = path.to_string_lossy().replace('\\', "/");
        let source_path = file.to_string_lossy().replace('\\', "/");
        source.push_str(&format!(
            "    ({key:?}, include_bytes!({source_path:?})),\n"
        ));
    }
    source.push_str("];\n");

    fs::write(out, source)?;
    Ok(())
}

/// Walks the `dir` and grabs all the files that it needs.
fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), Box<dyn error::Error>> {
    let entries = fs::read_dir(dir)?;

    for entry in entries {
        let path = entry?.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            collect(&path, files)?;
        } else if meta.is_file() {
            files.push(path);
        }
    }
    Ok(())
}
