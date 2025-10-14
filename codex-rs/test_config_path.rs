use std::path::PathBuf;

// Copy the find_codex_home function logic
fn find_codex_home() -> std::io::Result<PathBuf> {
    if let Ok(val) = std::env::var("CODEX_HOME")
        && !val.is_empty()
    {
        return PathBuf::from(val).canonicalize();
    }

    let mut p = dirs::home_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not find home directory",
        )
    })?;
    p.push(".codex-local");
    Ok(p)
}

fn main() {
    match find_codex_home() {
        Ok(path) => {
            println!("✅ Config path: {}", path.display());
            let config_file = path.join("config.toml");
            if config_file.exists() {
                println!("✅ Config file exists: {}", config_file.display());
            } else {
                println!("❌ Config file does not exist: {}", config_file.display());
            }
        }
        Err(e) => println!("❌ Error: {}", e),
    }
}
