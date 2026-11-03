//! Signed updates from GitHub Releases.
//!
//! Each release publishes `latest.json`, signed with the maintainer's Ed25519 key. The
//! app accepts it only when the signature verifies and the version is newer, then checks
//! the archive's size and SHA-256, and installs only a bundle whose code signature meets
//! the running app's designated requirement. That keeps the Accessibility permission and
//! means a stolen update key alone cannot replace the app.
use anyhow::{Context, Result, bail, ensure};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs,
    io::{Read, Write},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub const MANIFEST_URL: &str =
    "https://github.com/sxxrfx/infotransfer/releases/latest/download/latest.json";
const DOWNLOAD_PREFIX: &str = "https://github.com/sxxrfx/infotransfer/releases/download/";
const PUBLIC_KEY: &str = include_str!("../resources/update-public-key.txt");
const MAX_MANIFEST: u64 = 64 * 1024;
const MAX_ARCHIVE: u64 = 300_000_000;

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    /// Ed25519 signature over `message()`, hex encoded.
    pub signature: String,
}
impl Release {
    /// Everything the app acts on is signed, including the notes it displays.
    pub fn message(&self) -> String {
        format!(
            "infotransfer-update-v1\n{}\n{}\n{}\n{}\n{}",
            self.version, self.url, self.sha256, self.size, self.notes
        )
    }
    pub fn sign(&mut self, key: &SigningKey) {
        self.signature = hex::encode(key.sign(self.message().as_bytes()).to_bytes());
    }
    pub fn verify(&self, key: &VerifyingKey) -> Result<()> {
        let bytes: [u8; 64] = hex::decode(&self.signature)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .context("Update signature is malformed")?;
        key.verify_strict(self.message().as_bytes(), &Signature::from_bytes(&bytes))
            .ok()
            .context("Update signature is not valid")?;
        ensure!(
            self.url.starts_with(DOWNLOAD_PREFIX) && self.url.ends_with(".zip"),
            "Update points outside InfoTransfer's releases"
        );
        ensure!(
            self.sha256.len() == 64 && self.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Update checksum is malformed"
        );
        ensure!(
            (1..=MAX_ARCHIVE).contains(&self.size),
            "Update size is out of range"
        );
        version(&self.version).context("Update version is malformed")?;
        Ok(())
    }
}

pub fn public_key() -> VerifyingKey {
    let bytes: [u8; 32] = hex::decode(PUBLIC_KEY.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .expect("embedded update key is 32 hex bytes");
    VerifyingKey::from_bytes(&bytes).expect("embedded update key is valid")
}

/// Plain `major.minor.patch`; anything else is rejected rather than guessed.
pub fn version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.').map(|part| {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    });
    let parsed = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(parsed)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((version(candidate), version(current)), (Some(a), Some(b)) if a > b)
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(timeout)
        .user_agent(concat!("InfoTransfer/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// The newest verified release, if it is newer than this build.
pub fn check() -> Result<Option<Release>> {
    let response = client(Duration::from_secs(30))?
        .get(MANIFEST_URL)
        .send()?
        .error_for_status()
        .context("No update information is published yet")?;
    let mut bytes = Vec::new();
    response.take(MAX_MANIFEST + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_MANIFEST,
        "Update information is too large"
    );
    let release: Release =
        serde_json::from_slice(&bytes).context("Update information is malformed")?;
    release.verify(&public_key())?;
    Ok(is_newer(&release.version, current_version()).then_some(release))
}

/// Downloads into `directory` and returns the verified archive.
pub fn download(
    release: &Release,
    directory: &Path,
    mut progress: impl FnMut(u64, u64) -> bool,
) -> Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let target = directory.join(format!("InfoTransfer-{}.zip", release.version));
    let partial = target.with_extension("part");
    let mut response = client(Duration::from_secs(3600))?
        .get(&release.url)
        .send()?
        .error_for_status()?;
    let mut file = fs::File::create(&partial)?;
    let mut hash = Sha256::new();
    let mut received = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if !progress(received, release.size) {
            drop(file);
            let _ = fs::remove_file(&partial);
            bail!("Update download canceled");
        }
        let n = response.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        received += n as u64;
        ensure!(received <= release.size, "Update is larger than published");
        hash.update(&buffer[..n]);
        file.write_all(&buffer[..n])?;
    }
    file.sync_all()?;
    if received != release.size || hex::encode(hash.finalize()) != release.sha256 {
        let _ = fs::remove_file(&partial);
        bail!("Update download failed its integrity check");
    }
    fs::rename(&partial, &target)?;
    Ok(target)
}

/// The .app bundle this process runs from, when it runs from one.
pub fn running_bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
    let bundle = executable.parent()?.parent()?.parent()?;
    (executable.parent()?.file_name()? == "MacOS"
        && bundle.extension().is_some_and(|extension| extension == "app"))
    .then(|| bundle.to_path_buf())
}

fn checked(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    ensure!(output.status.success(), "{:?} failed", command.get_program());
    Ok(String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr))
}

fn designated_requirement(bundle: &Path) -> Result<String> {
    let details = checked(Command::new("/usr/bin/codesign").arg("-d").arg("-r-").arg(bundle))?;
    details
        .lines()
        .find_map(|line| line.split_once("designated => ").map(|(_, rest)| rest.trim()))
        .map(str::to_owned)
        .context("The running app has no designated requirement")
}

fn bundle_version(bundle: &Path) -> Result<String> {
    let value = checked(
        Command::new("/usr/libexec/PlistBuddy")
            .arg("-c")
            .arg("Print :CFBundleShortVersionString")
            .arg(bundle.join("Contents/Info.plist")),
    )?;
    Ok(value.trim().to_owned())
}

/// Replaces `bundle` with the archive's app and returns once the swap is done. The
/// running process keeps its open files; the new version starts on relaunch.
pub fn install(archive: &Path, release: &Release, bundle: &Path) -> Result<()> {
    let requirement = designated_requirement(bundle)?;
    ensure!(
        !requirement.contains("cdhash"),
        "This copy has an ad-hoc signature. Rebuild it from source to update."
    );
    let parent = bundle.parent().context("App location")?;
    let staging = parent.join(".InfoTransfer.update");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir(&staging).context(
        "InfoTransfer cannot write to its folder. Move it to Applications, or install the update manually.",
    )?;
    let result = (|| {
        checked(
            Command::new("/usr/bin/ditto")
                .args(["-x", "-k"])
                .arg(archive)
                .arg(&staging),
        )
        .context("The update archive could not be unpacked")?;
        let candidate = staging.join("InfoTransfer.app");
        ensure!(candidate.is_dir(), "The update does not contain InfoTransfer.app");
        checked(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&candidate),
        )
        .context("The update's code signature is broken")?;
        checked(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--strict"])
                .arg(format!("-R={requirement}"))
                .arg(&candidate),
        )
        .context(
            "The update is signed by a different identity than this copy. If you built \
             InfoTransfer yourself, rebuild it from source to update.",
        )?;
        ensure!(
            bundle_version(&candidate)? == release.version,
            "The update's version does not match its release"
        );
        let _ = Command::new("/usr/bin/xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(&candidate)
            .status();
        swap(&candidate, bundle)
    })();
    // After a swap the old version sits in staging; either way, staging goes.
    let _ = fs::remove_dir_all(&staging);
    result
}

fn swap(new: &Path, old: &Path) -> Result<()> {
    let from = CString::new(new.as_os_str().as_bytes())?;
    let to = CString::new(old.as_os_str().as_bytes())?;
    // Both paths are valid C strings on the same volume; RENAME_SWAP is atomic.
    let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) };
    ensure!(
        status == 0,
        "InfoTransfer could not be replaced: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

/// Opens `bundle` again once this process has exited. The caller quits right after.
pub fn relaunch_after_exit(bundle: &Path) -> Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"while /bin/kill -0 "$1" 2>/dev/null; do /bin/sleep 0.2; done; /usr/bin/open "$2""#)
        .arg("relaunch")
        .arg(std::process::id().to_string())
        .arg(bundle)
        .spawn()
        .context("Could not schedule the relaunch")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(key: &SigningKey) -> Release {
        let mut release = Release {
            version: "0.3.0".into(),
            notes: "Faster start.".into(),
            url: format!("{DOWNLOAD_PREFIX}v0.3.0/InfoTransfer-0.3.0-arm64.zip"),
            sha256: "ab".repeat(32),
            size: 1234,
            signature: String::new(),
        };
        release.sign(key);
        release
    }
    #[test]
    fn versions_compare_numerically_and_reject_other_forms() {
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        for bad in ["0.2", "0.2.0.1", "v0.2.0", "0.2.0-beta", "0..1", ""] {
            assert_eq!(version(bad), None, "{bad}");
            assert!(!is_newer(bad, "0.0.1"));
        }
    }
    #[test]
    fn only_the_signed_manifest_verifies() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let good = release(&key);
        assert!(good.verify(&key.verifying_key()).is_ok());
        let other = SigningKey::from_bytes(&[8; 32]);
        assert!(good.verify(&other.verifying_key()).is_err());
        for tamper in [
            |r: &mut Release| r.version = "0.4.0".into(),
            |r: &mut Release| r.sha256 = "cd".repeat(32),
            |r: &mut Release| r.size += 1,
            |r: &mut Release| r.notes = "Install me.".into(),
            |r: &mut Release| r.url = r.url.replace("0.3.0-arm64", "0.3.0-x"),
        ] {
            let mut changed = good.clone();
            tamper(&mut changed);
            assert!(changed.verify(&key.verifying_key()).is_err());
        }
    }
    #[test]
    fn signed_manifests_must_still_point_at_this_project() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let mut elsewhere = release(&key);
        elsewhere.url = "https://example.com/InfoTransfer.zip".into();
        elsewhere.sign(&key);
        assert!(elsewhere.verify(&key.verifying_key()).is_err());
    }
    #[test]
    fn the_embedded_key_parses() {
        let _ = public_key();
    }
}
