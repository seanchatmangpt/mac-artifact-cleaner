use blake3;
use std::fs::File;
use std::io::{Read, Result};
use std::path::Path;

pub fn generate_manifest(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 65536];

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}
