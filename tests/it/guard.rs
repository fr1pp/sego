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

#[test]
fn every_test_file_is_declared() -> io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it");
    let main = fs::read_to_string(root.join("main.rs"))?;
    let support =
        fs::read_to_string(root.join("support/mod.rs")).unwrap_or_default();

    let mut files = Vec::new();
    rust_files(&root, &mut files)?;

    let mut undeclared = Vec::new();
    for file in files {
        let relative = file.strip_prefix(&root).unwrap_or(&file);
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
            [dir, name] if dir == "support" && name == "mod.rs" => {
                declares(&main, "support")
            }
            [dir, _] if dir == "support" => declares(&support, &stem),
            _ => false,
        };
        if !declared {
            undeclared.push(relative.display().to_string());
        }
    }

    assert!(
        undeclared.is_empty(),
        "test files no `mod` line declares: {undeclared:?}"
    );
    Ok(())
}
