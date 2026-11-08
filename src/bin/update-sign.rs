//! Release signing for the in-app updater.
//!
//!   update-sign keygen <private-key-file>
//!   update-sign manifest <zip> <version> <notes-file> <private-key-file> <output.json>
//!
//! A key file of `-` reads the key from standard input, so it never touches the disk.
//!
//! The private key is 32 random bytes, hex encoded, readable only by its owner. Keep a
//! backup: without it, installed copies cannot verify future updates.
use anyhow::{Context, Result, bail, ensure};
use ed25519_dalek::SigningKey;
use infotransfer::update::{self, Release};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

fn read_key(path: &Path) -> Result<SigningKey> {
    let text = if path == Path::new("-") {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        text
    } else {
        fs::read_to_string(path)?
    };
    let bytes: [u8; 32] = hex::decode(text.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .context("Private key must be 32 hex bytes")?;
    Ok(SigningKey::from_bytes(&bytes))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["keygen", path] => {
            let path = Path::new(path);
            ensure!(!path.exists(), "{} already exists", path.display());
            let mut seed = [0u8; 32];
            fs::File::open("/dev/urandom")?.read_exact(&mut seed)?;
            let key = SigningKey::from_bytes(&seed);
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)?
                .write_all(format!("{}\n", hex::encode(seed)).as_bytes())?;
            println!("{}", hex::encode(key.verifying_key().to_bytes()));
        }
        ["manifest", archive, version, notes, key, output] => {
            ensure!(
                update::version(version).is_some(),
                "Version must be major.minor.patch"
            );
            let key = read_key(Path::new(key))?;
            ensure!(
                key.verifying_key() == update::public_key(),
                "This private key does not match resources/update-public-key.txt"
            );
            let bytes = fs::read(archive)?;
            let name = Path::new(archive)
                .file_name()
                .and_then(|name| name.to_str())
                .context("Archive name")?;
            let mut release = Release {
                version: version.to_owned(),
                notes: fs::read_to_string(notes)?.trim().to_owned(),
                url: format!(
                    "https://github.com/sxxrfx/infotransfer/releases/download/v{version}/{name}"
                ),
                sha256: hex::encode(Sha256::digest(&bytes)),
                size: bytes.len() as u64,
                signature: String::new(),
            };
            release.sign(&key);
            release.verify(&update::public_key())?;
            fs::write(output, serde_json::to_string_pretty(&release)? + "\n")?;
            println!("{}", release.sha256);
        }
        _ => bail!(
            "usage: update-sign keygen <key-file> | manifest <zip> <version> <notes-file> <key-file> <output.json>"
        ),
    }
    Ok(())
}
