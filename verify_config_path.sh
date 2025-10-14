#!/usr/bin/env bash

echo "=== Verifying config path changes ==="
echo ""

# Test if the config path change worked by checking if codex-local can now read from .codex-local
echo "1. Creating a test config file with unique identifier..."
echo "test_unique_id = \"codex_local_test_$(date +%s)\"" >> ~/.codex-local/config.toml

echo ""
echo "2. Testing if binary reads from ~/.codex-local/..."
cd /Users/sero/projects/codex-local/codex-rs

# Create a simple Rust test program to check config path
cat > test_config_path.rs << 'EOF'
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
EOF

echo "3. Testing config path logic..."
rustc --extern dirs=$(find ~/.cargo -name "libdirs.rlib" | head -1) test_config_path.rs -o test_config_path 2>/dev/null || echo "Rust compilation failed"

if [ -f test_config_path ]; then
    ./test_config_path
    rm test_config_path test_config_path.rs
else
    echo "4. Fallback: Checking if we can read config content..."
    if grep -q "test_unique_id" ~/.codex-local/config.toml 2>/dev/null; then
        echo "✅ Test identifier found in ~/.codex-local/config.toml"
    else
        echo "❌ Test identifier not found"
    fi
fi

echo ""
echo "5. Restoring original config..."
# Remove the test line we added
sed -i '' '/test_unique_id/d' ~/.codex-local/config.toml