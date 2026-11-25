use std::fmt::Write as _;

use flate2::{Compression, write::GzEncoder};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use super::*;

#[tokio::test]
async fn installs_a_verified_archive_and_reuses_the_cache() {
    let server = MockServer::start().await;
    let bytes = fixture_archive(PROGRAM);
    let digest = hex_digest(&bytes);
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
        .expect(1)
        .mount(&server)
        .await;
    let cache = tempfile::tempdir().unwrap();
    let path = cache.path().to_owned();
    let url = server.uri();
    let artifact = || Archive {
        url: &url,
        sha256: &digest,
    };
    let installed = Runtime::acquire_at(path.clone(), artifact()).unwrap();
    assert_eq!(fs::read(installed.program()).unwrap(), b"fake runtime");
    drop(installed);
    let cached = Runtime::acquire_at(path, artifact()).unwrap();
    assert_eq!(fs::read(cached.program()).unwrap(), b"fake runtime");
}

#[tokio::test]
async fn rejects_a_verified_archive_without_the_pinned_executable() {
    let server = MockServer::start().await;
    let bytes = fixture_archive("documentation.txt");
    let digest = hex_digest(&bytes);
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
        .expect(1)
        .mount(&server)
        .await;
    let cache = tempfile::tempdir().unwrap();
    let artifact = Archive {
        url: &server.uri(),
        sha256: &digest,
    };
    assert!(matches!(
        Runtime::acquire_at(cache.path().into(), artifact),
        Err(CopilotError::Archive)
    ));
    assert!(!cache.path().join(VERSION).exists());
}

#[tokio::test]
async fn does_not_cache_an_http_error_as_a_runtime_archive() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403).set_body_string("access denied"))
        .expect(1)
        .mount(&server)
        .await;
    let cache = tempfile::tempdir().unwrap();
    let artifact = Archive {
        url: &server.uri(),
        sha256: SHA256,
    };
    assert!(matches!(
        Runtime::acquire_at(cache.path().into(), artifact),
        Err(CopilotError::Download)
    ));
    assert!(!cache.path().join(VERSION).exists());
}

#[tokio::test]
async fn retries_install_into_an_incomplete_version_directory() {
    let server = MockServer::start().await;
    let bytes = fixture_archive(PROGRAM);
    let digest = hex_digest(&bytes);
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
        .expect(1)
        .mount(&server)
        .await;
    let cache = tempfile::tempdir().unwrap();
    let partial = cache.path().join(VERSION);
    fs::create_dir(&partial).unwrap();
    fs::write(partial.join("incomplete"), b"").unwrap();
    let artifact = Archive {
        url: &server.uri(),
        sha256: &digest,
    };
    let installed = Runtime::acquire_at(cache.path().into(), artifact).unwrap();
    assert!(installed.program().is_file());
    assert!(!partial.join("incomplete").exists());
}

#[tokio::test]
async fn does_not_install_an_archive_with_a_bad_checksum() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("untrusted archive"))
        .expect(1)
        .mount(&server)
        .await;
    let cache = tempfile::tempdir().unwrap();
    let artifact = Archive {
        url: &server.uri(),
        sha256: SHA256,
    };
    let error = Runtime::acquire_at(cache.path().into(), artifact)
        .err()
        .unwrap();
    assert!(matches!(error, CopilotError::Checksum));
    assert!(!cache.path().join(VERSION).exists());
}

#[test]
fn prunes_only_old_version_directories() {
    let cache = tempfile::tempdir().unwrap();
    for (index, version) in [
        "0.0.1",
        "0.0.2",
        "0.0.3",
        "0.0.4",
        "0.0.5",
        VERSION,
        "do-not-remove",
    ]
    .into_iter()
    .enumerate()
    {
        let path = cache.path().join(version);
        fs::create_dir(&path).unwrap();
        File::create(path.join(".used"))
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(index as u64))
            .unwrap();
    }
    let path = cache.path().to_owned();
    let program = path.join(VERSION).join(PROGRAM);
    fs::create_dir_all(program.parent().unwrap()).unwrap();
    File::create(program).unwrap();
    let runtime = Runtime::acquire_at(
        path,
        Archive {
            url: "",
            sha256: "",
        },
    )
    .unwrap();
    runtime.prune().unwrap();
    assert!(!cache.path().join("0.0.1").exists());
    assert!(cache.path().join("0.0.2").exists());
    assert!(cache.path().join("do-not-remove").exists());
}

fn fixture_archive(path: &str) -> Vec<u8> {
    let compressed = GzEncoder::new(Vec::new(), Compression::fast());
    let mut tar = tar::Builder::new(compressed);
    let mut header = tar::Header::new_gnu();
    let data = b"fake runtime";
    header.set_mode(0o755);
    header.set_size(data.len() as u64);
    tar.append_data(&mut header, path, &data[..]).unwrap();
    tar.into_inner().unwrap().finish().unwrap()
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut text, value| {
            write!(&mut text, "{value:02x}").unwrap();
            text
        })
}
