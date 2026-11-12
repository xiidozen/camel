//! Ports of the read lock tests of `org.apache.camel.component.file` and its `strategy`
//! package.

use crate::support::{await_until, ContextTestSupport, TestDirectory};
use camel_core::api::exchange_constants::{FILE_LENGTH, FILE_NAME, FILE_PATH};
use camel_core::prelude::*;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

async fn wait(condition: impl Fn() -> bool) {
    await_until(Duration::from_secs(5), condition).await;
}

/// The lines of the slow file.
fn lines() -> Vec<String> {
    (0..20).map(|i| format!("Line {i}\n")).collect()
}

/// Writes the lines to the file, waiting 50 millis after each (a file being written).
async fn write_slow_file(path: std::path::PathBuf) {
    let mut f = std::fs::File::create(path).unwrap();
    for line in lines() {
        f.write_all(line.as_bytes()).unwrap();
        f.flush().unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn assert_lines(dir: &TestDirectory, name: &str) {
    let content = dir.read(name).unwrap();
    assert_eq!(
        lines().concat(),
        content,
        "There should be 20 lines in the file"
    );
}

/// Ports of `FileChangedReadLockTest`, `FileChangedReadLockMarkerFileFalseTest`,
/// `FileChangedReadLockTimeoutTest` and `FileChangedReadLockLoggingLevelTest`: the slow file is
/// taken once written.
#[tokio::test]
async fn test_changed_read_lock() {
    for option in [
        "",
        "&readLockMarkerFile=false",
        "&readLockTimeout=2000",
        "&readLockLoggingLevel=DEBUG",
    ] {
        let dir = TestDirectory::new("FileChangedReadLockTest");
        std::fs::create_dir_all(dir.test_file("in")).unwrap();
        let (from_uri, out) = (
            dir.file_uri(&format!(
                "in?initialDelay=0&delay=10&readLock=changed&readLockCheckInterval=100{option}"
            )),
            dir.file_uri("out"),
        );
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&from_uri).to(&out).to("mock:result"));
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_message_count(1);
        mock.expected_file_exists(dir.test_file("out/slowfile.dat"), None);
        mock.expected_header_received(FILE_LENGTH, lines().concat().len() as i64);
        write_slow_file(dir.test_file("in/slowfile.dat")).await;
        mock.assert_is_satisfied().await;
        assert_lines(&dir, "out/slowfile.dat");
    }
}

/// Port of `FileChangedReadLockMinAgeTest` (the file is taken once a second old).
#[tokio::test]
async fn test_changed_read_lock_min_age() {
    let dir = TestDirectory::new("FileChangedReadLockMinAgeTest");
    std::fs::create_dir_all(dir.test_file("in")).unwrap();
    let (from_uri, out) = (
        dir.file_uri("in?initialDelay=0&delay=10&readLock=changed&readLockCheckInterval=100&readLockMinAge=1000&readLockTimeout=1500"),
        dir.file_uri("out"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&from_uri).to(&out).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("out/slowfile.dat"), None);
    let start = Instant::now();
    write_slow_file(dir.test_file("in/slowfile.dat")).await;
    mock.assert_is_satisfied().await;
    assert!(
        start.elapsed() > Duration::from_millis(1500),
        "{:?}",
        start.elapsed()
    );
    assert_lines(&dir, "out/slowfile.dat");
}

/// Ports of `FileChangedReadLockZeroTimeoutTest` and `FileChangedZeroLengthReadLockTest`.
#[tokio::test]
async fn test_changed_read_lock_zero_timeout_and_length() {
    let dir = TestDirectory::new("FileChangedReadLockZeroTimeoutTest");
    let (from_uri, out) = (
        dir.file_uri("in?initialDelay=0&delay=10&readLock=changed&readLockCheckInterval=50&readLockTimeout=0"),
        dir.file_uri("out"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&from_uri).to(&out).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    for (name, body) in [
        ("hello1.txt", "Hello World"),
        ("hello2.txt", "Hello Again World"),
    ] {
        mock.reset();
        mock.expected_message_count(1);
        mock.expected_file_exists(dir.test_file(&format!("out/{name}")), None);
        t.template
            .send_body_and_header(&dir.file_uri("in"), body, FILE_NAME, name)
            .await
            .unwrap();
        mock.assert_is_satisfied().await;
    }

    let dir = TestDirectory::new("FileChangedZeroLengthReadLockTest");
    std::fs::create_dir_all(dir.test_file("in")).unwrap();
    let (from_uri, out) = (
        dir.file_uri("in?initialDelay=0&delay=10&readLock=changed&readLockCheckInterval=100&readLockMinLength=0"),
        dir.file_uri("out"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&from_uri).to(&out).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("out/zerofile.dat"), None);
    std::fs::write(dir.test_file("in/zerofile.dat"), b"").unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `MarkerFileExclusiveReadLockStrategyTest` (the files routed on a pool of tasks).
#[tokio::test]
async fn test_marker_file_multithreaded_locking() {
    let dir = TestDirectory::new("MarkerFileExclusiveReadLockStrategyTest");
    std::fs::create_dir_all(dir.test_file("in")).unwrap();
    let processed = Arc::new(AtomicUsize::new(0));
    let (from_uri, out, p) = (
        dir.file_uri("in?readLock=markerFile&initialDelay=1000&delay=2000"),
        dir.file_uri("out"),
        processed.clone(),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let p = p.clone();
        r.add(
            from(&from_uri)
                .on_completion(move |c| {
                    c.process_fn(move |_| {
                        p.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })
                .threads_pool(5)
                .to(&out)
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(2);
    mock.expected_file_exists(dir.test_file("out/file1.dat"), None);
    mock.expected_file_exists(dir.test_file("out/file2.dat"), None);
    let content = lines().concat();
    dir.write("in/file1.dat", &content);
    dir.write("in/file2.dat", &content);
    mock.assert_is_satisfied().await;
    assert_lines(&dir, "out/file1.dat");
    assert_lines(&dir, "out/file2.dat");
    wait(|| processed.load(Ordering::SeqCst) == 2).await;
    for f in [
        "in/file1.dat.camelLock",
        "in/file2.dat.camelLock",
        "in/file1.dat",
        "in/file2.dat",
    ] {
        let f = dir.test_file(f);
        wait(|| !f.exists()).await;
        assert!(!f.exists(), "File {} should not exist", f.display());
    }
    assert_eq!(2, processed.load(Ordering::SeqCst));
}

/// Port of `MarkerFileExclusiveReadLockStrategyUnlockTest`: the lock of a polled file is
/// released too.
#[tokio::test]
async fn test_marker_file_unlock() {
    let dir = TestDirectory::new("MarkerFileExclusiveReadLockStrategyUnlockTest");
    std::fs::create_dir_all(dir.test_file("input-a")).unwrap();
    std::fs::create_dir_all(dir.test_file("input-b")).unwrap();
    let (a, b) = (
        dir.file_uri("input-a?fileName=file1.dat&readLock=markerFile&initialDelay=0&delay=2000"),
        dir.file_uri("input-b?fileName=file2.dat&readLock=markerFile&initialDelay=0&delay=2000"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&a).poll_enrich(&b, -1).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    dir.write("input-a/file1.dat", "File-1");
    dir.write("input-b/file2.dat", "File-2");
    mock.assert_is_satisfied().await;
    for f in ["input-a/file1.dat.camelLock", "input-b/file2.dat.camelLock"] {
        let f = dir.test_file(f);
        wait(|| !f.exists()).await;
        assert!(!f.exists(), "{}", f.display());
    }
}

/// Port of `FileNoOpLockFileTest`: the marker file exists while routing, and is deleted with
/// noop too.
#[tokio::test]
async fn test_noop_lock_file() {
    let dir = TestDirectory::new("FileNoOpLockFileTest");
    let (locked, not_locked) = (
        dir.file_uri("locked/?initialDelay=0&delay=10&noop=true&readLock=markerFile"),
        dir.file_uri("notlocked/?initialDelay=0&delay=10&noop=true&readLock=none"),
    );
    let lock_of = |kind: &str| dir.test_file(&format!("{kind}/report.txt.camelLock"));
    let (l, n) = (lock_of("locked"), lock_of("notlocked"));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let check = |lock: std::path::PathBuf, expected: bool| {
            move |_: &mut Exchange| {
                assert_eq!(expected, lock.exists(), "Lock file {}", lock.display());
                Ok(())
            }
        };
        r.add(
            from(&locked)
                .process_fn(check(l.clone(), true))
                .to("mock:report"),
        );
        r.add(
            from(&not_locked)
                .process_fn(check(n.clone(), false))
                .to("mock:report"),
        );
    })
    .await;
    let mock = t.mock("mock:report");
    for (kind, body) in [
        ("locked", "Hello Locked"),
        ("notlocked", "Hello Not Locked"),
    ] {
        mock.reset();
        mock.expected_bodies_received([body]);
        t.template
            .send_body_and_header(&dir.file_uri(kind), body, FILE_NAME, "report.txt")
            .await
            .unwrap();
        mock.assert_is_satisfied().await;
        let lock = lock_of(kind);
        wait(|| !lock.exists()).await;
        assert!(!lock.exists(), "Lock file should not exists");
    }
}

/// Port of `FileRenameReadLockMustUseMarkerFileTest`.
#[tokio::test]
async fn test_rename_read_lock_uses_marker_file() {
    let dir = TestDirectory::new("FileRenameReadLockMustUseMarkerFileTest");
    let uri = dir.file_uri("?readLock=rename&initialDelay=0&delay=10");
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(
                from(&uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .process_fn(|e| {
                        let name = format!(
                            "{}.camelLock",
                            e.message()
                                .header(FILE_PATH)
                                .map(|v| v.to_string())
                                .unwrap_or_default()
                        );
                        assert!(
                            std::path::Path::new(&name).exists(),
                            "Lock file should exist: {name}"
                        );
                        Ok(())
                    })
                    .convert_body_to("String")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Bye World"]);
    mock.message_matches(0, header(FILE_NAME).is_equal_to("bye.txt"));
    t.template
        .send_body_and_header(&dir.file_uri(""), "Bye World", FILE_NAME, "bye.txt")
        .await
        .unwrap();
    t.context.start_route("foo").await.unwrap();
    mock.assert_is_satisfied().await;
    let lock = dir.test_file("bye.txt.camelLock");
    wait(|| !lock.exists()).await;
    assert!(!lock.exists());
}

/// Ports of `FileMarkerFileDeleteOldLockFilesTest` and
/// `FileMarkerFileRecursiveDoNotDeleteOldLockFilesTest`: a marker file is never consumed, an
/// orphan one keeps its file locked unless deleted on start.
#[tokio::test]
async fn test_marker_file_old_lock_files() {
    for (option, expected) in [
        ("?initialDelay=0&delay=10", vec!["Bye World", "Hello World", "Goodday World", "New World"]),
        (
            "?initialDelay=0&delay=10&readLock=markerFile&readLockDeleteOrphanLockFiles=false&recursive=true",
            vec!["Bye World", "New World"],
        ),
        ("?initialDelay=0&delay=10&readLock=markerFile&recursive=true", vec!["Bye World", "Hello World", "Goodday World", "New World"]),
    ] {
        let dir = TestDirectory::new("FileMarkerFileDeleteOldLockFilesTest");
        let uri = dir.file_uri(option);
        let t = ContextTestSupport::new();
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(from(&uri).route_id("foo").auto_startup(false).convert_body_to("String").to("mock:result"));
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        for (name, body) in [
            ("hello.txt.camelLock", "locked"),
            ("bye.txt", "Bye World"),
            ("hello.txt", "Hello World"),
            ("foo/gooday.txt.camelLock", "locked"),
            ("foo/gooday.txt", "Goodday World"),
            ("new.txt", "New World"),
        ] {
            // the files of a sub directory only with recursive
            if name.starts_with("foo/") && !option.contains("recursive") {
                continue;
            }
            t.template
                .send_body_and_header(&dir.file_uri(""), body, FILE_NAME, name)
                .await
                .unwrap();
        }
        let expected: Vec<&str> = expected
            .into_iter()
            .filter(|b| *b != "Goodday World" || option.contains("recursive"))
            .collect();
        let mock = t.mock("mock:result");
        mock.expected_bodies_received_in_any_order(expected.clone());
        t.context.start_route("foo").await.unwrap();
        mock.assert_is_satisfied().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(expected.len(), mock.received_counter(), "{option}");
    }
}

/// Port of `FileChangedReadLockMinAgeShortCircuitTest`: a file younger than the min age is not
/// taken before the next check.
#[tokio::test]
async fn test_changed_read_lock_min_age_not_acquired() {
    let dir = TestDirectory::new("FileChangedReadLockMinAgeShortCircuitTest");
    dir.write("in/file.dat", "Line");
    let (from_uri, out) = (
        dir.file_uri("in?initialDelay=500&delay=10&readLock=changed&readLockMinAge=1000&readLockCheckInterval=10000&readLockTimeout=20000"),
        dir.file_uri("out"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&from_uri).to(&out).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(0);
    tokio::time::sleep(Duration::from_millis(2000)).await;
    mock.assert_is_satisfied().await;
    let stop = Instant::now();
    t.context.stop().await.unwrap();
    assert!(
        stop.elapsed() < Duration::from_secs(5),
        "{:?}",
        stop.elapsed()
    );
}
