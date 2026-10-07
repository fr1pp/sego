use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        entries.push(entry?.path());
    }
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn declares(source: &str, module: &str) -> bool {
    source.lines().any(|line| {
        let line = line.trim();
        let line = line.strip_prefix("pub ").unwrap_or(line);
        line.strip_prefix("mod ")
            .and_then(|rest| rest.strip_suffix(';'))
            .is_some_and(|name| name.trim() == module)
    })
}

/// The `.rs` files under `root` that no `mod` line declares: a file in
/// `root` in `main.rs`, a directory module `<d>/mod.rs` in `main.rs`, and
/// each `<d>/<f>.rs` in `<d>/mod.rs`.
fn undeclared(root: &Path) -> io::Result<Vec<String>> {
    let main = fs::read_to_string(root.join("main.rs"))?;
    let mut files = Vec::new();
    rust_files(root, &mut files)?;

    let mut missing = Vec::new();
    for file in files {
        let relative = file.strip_prefix(root).unwrap_or(&file);
        let parts: Vec<_> = relative
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect();
        let stem = file
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let declared = match parts.as_slice() {
            [name] if name == "main.rs" => true,
            [_] => declares(&main, &stem),
            [dir, name] if name == "mod.rs" => declares(&main, dir),
            [dir, _] => {
                let parent = fs::read_to_string(root.join(dir).join("mod.rs"))
                    .unwrap_or_default();
                declares(&parent, &stem)
            }
            _ => false,
        };
        if !declared {
            missing.push(relative.display().to_string());
        }
    }
    Ok(missing)
}

#[test]
fn every_test_file_is_declared() -> io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it");
    let missing = undeclared(&root)?;
    assert!(
        missing.is_empty(),
        "test files no `mod` line declares: {missing:?}"
    );
    Ok(())
}

#[test]
fn the_guard_checks_directory_modules_and_their_files() -> io::Result<()> {
    let root = std::env::temp_dir()
        .join(format!("sego-guard-{}", std::process::id()));
    let write = |name: &str, text: &str| -> io::Result<()> {
        let path = root.join(name);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, text)
    };
    write("main.rs", "mod a;\nmod b;\nmod c;\n")?;
    write("a.rs", "")?;
    write("b/mod.rs", "mod x;\npub mod y;\n")?;
    write("b/x.rs", "")?;
    write("b/y.rs", "")?;
    write("b/z.rs", "")?;
    write("d/mod.rs", "")?;
    write("d/w.rs", "")?;
    write("c/mod.rs", "")?;
    write("c/deep/mod.rs", "")?;
    let missing = undeclared(&root);
    fs::remove_dir_all(&root)?;
    assert_eq!(missing?, ["b/z.rs", "c/deep/mod.rs", "d/mod.rs", "d/w.rs"]);
    Ok(())
}
