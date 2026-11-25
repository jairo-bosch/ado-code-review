use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use curl::easy::Easy;
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use super::CopilotError;

#[cfg(test)]
mod tests;

const VERSION: &str = "1.0.115";
const SHA256: &str = "e4185fa9581774b9479241583a25ddbfd30e91294f48d2379279dc04c0b63267";
const PROGRAM: &str = "package/prebuilds/linux-x64/copilot-runtime";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(5);
const MAX_ARCHIVE_BYTES: u64 = 180_000_000;
const MAX_VERSIONS: usize = 5;
const MAX_REDIRECTS: u32 = 5;
const HEX_WIDTH: usize = 2;

pub(super) struct Runtime {
    root: PathBuf,
    program: PathBuf,
    _lock: File,
}

#[derive(Clone, Copy)]
struct Archive<'a> {
    url: &'a str,
    sha256: &'a str,
}

impl Runtime {
    pub(super) fn acquire() -> Result<Self, CopilotError> {
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err(CopilotError::Platform);
        }
        let url = format!(
            "https://github.com/github/copilot-cli/releases/download/v{VERSION}/github-copilot-{VERSION}-linux-x64.tgz"
        );
        Self::acquire_at(
            cache_dir()?,
            Archive {
                url: &url,
                sha256: SHA256,
            },
        )
    }

    fn acquire_at(root: PathBuf, archive: Archive<'_>) -> Result<Self, CopilotError> {
        fs::create_dir_all(&root)?;
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(root.join(".lock"))?;
        lock.lock()?;
        let destination = root.join(VERSION);
        let program = destination.join(PROGRAM);
        if !program.is_file() {
            install(&root, &destination, archive)?;
        }
        mark_used(&destination)?;
        Ok(Self {
            root,
            program,
            _lock: lock,
        })
    }

    pub(super) fn program(&self) -> &Path {
        &self.program
    }

    pub(super) fn prune(&self) -> Result<(), CopilotError> {
        let mut versions = version_dirs(&self.root)?;
        versions.sort_unstable_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        for (_, version) in versions.into_iter().skip(MAX_VERSIONS) {
            if version != self.root.join(VERSION) {
                fs::remove_dir_all(version)?;
            }
        }
        Ok(())
    }
}

fn cache_dir() -> Result<PathBuf, CopilotError> {
    let configured = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from);
    if let Some(dir) = configured.filter(|dir| dir.is_absolute()) {
        return Ok(dir.join("ado-code-review/copilot-runtime"));
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(".cache/ado-code-review/copilot-runtime"))
        .ok_or(CopilotError::CachePath)
}

fn install(root: &Path, destination: &Path, archive: Archive<'_>) -> Result<(), CopilotError> {
    let mut download = tempfile::tempfile_in(root)?;
    download_archive(&mut download, archive.url)?;
    download.rewind()?;
    verify(&mut download, archive.sha256)?;
    download.rewind()?;
    let extract = tempfile::Builder::new()
        .prefix(".extract-")
        .tempdir_in(root)?;
    tar::Archive::new(GzDecoder::new(download)).unpack(extract.path())?;
    if !extract.path().join(PROGRAM).is_file() {
        return Err(CopilotError::Archive);
    }
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    fs::rename(extract.path(), destination)?;
    Ok(())
}

fn download_archive(file: &mut File, url: &str) -> Result<(), CopilotError> {
    let mut curl = Easy::new();
    configure_download(&mut curl, url).map_err(|_| CopilotError::Download)?;
    let mut remaining = MAX_ARCHIVE_BYTES;
    let mut transfer = curl.transfer();
    transfer
        .write_function(|bytes| {
            let Some(left) = remaining.checked_sub(bytes.len() as u64) else {
                return Ok(0);
            };
            if file.write_all(bytes).is_err() {
                return Ok(0);
            }
            remaining = left;
            Ok(bytes.len())
        })
        .map_err(|_| CopilotError::Download)?;
    transfer.perform().map_err(|_| CopilotError::Download)
}

fn configure_download(curl: &mut Easy, url: &str) -> Result<(), curl::Error> {
    curl.url(url)?;
    curl.follow_location(true)?;
    curl.max_redirections(MAX_REDIRECTS)?;
    curl.timeout(DOWNLOAD_TIMEOUT)?;
    curl.max_filesize(MAX_ARCHIVE_BYTES)?;
    curl.fail_on_error(true)?;
    curl.ssl_verify_peer(true)?;
    curl.ssl_verify_host(true)
}

fn verify(file: &mut File, sha256: &str) -> Result<(), CopilotError> {
    const BUFFER_SIZE: usize = 8192;
    let mut hash = Sha256::new();
    let mut buffer = [0; BUFFER_SIZE];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(buffer.get(..count).ok_or(CopilotError::Checksum)?);
    }
    let (expected, remainder) = sha256.as_bytes().as_chunks::<HEX_WIDTH>();
    let actual = hash.finalize();
    if remainder.is_empty()
        && actual
            .iter()
            .copied()
            .map(Some)
            .eq(expected.iter().copied().map(parse_hex_byte))
    {
        Ok(())
    } else {
        Err(CopilotError::Checksum)
    }
}

fn parse_hex_byte(bytes: [u8; HEX_WIDTH]) -> Option<u8> {
    const HEX_BASE: u32 = 16;
    u8::from_str_radix(std::str::from_utf8(&bytes).ok()?, HEX_BASE).ok()
}

fn mark_used(path: &Path) -> io::Result<()> {
    File::create(path.join(".used"))?.set_modified(SystemTime::now())
}

fn version_dirs(root: &Path) -> io::Result<Vec<(SystemTime, PathBuf)>> {
    let mut versions = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && is_version(&entry.file_name()) {
            let path = entry.path();
            let timestamp = last_used(&entry)?;
            versions.push((timestamp, path));
        }
    }
    Ok(versions)
}

fn last_used(entry: &fs::DirEntry) -> io::Result<SystemTime> {
    match fs::metadata(entry.path().join(".used")) {
        Ok(metadata) => metadata.modified(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => entry.metadata()?.modified(),
        Err(error) => Err(error),
    }
}

fn is_version(name: &std::ffi::OsStr) -> bool {
    let Some(value) = name.to_str() else {
        return false;
    };
    let mut parts = value.split('.');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(major), Some(minor), Some(patch), None)
            if [major, minor, patch].iter().all(|part| part.parse::<u32>().is_ok())
    )
}
