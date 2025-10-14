#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sys::inotify::{Event, Inotify};

const IN_CREATE: u32 = 0x0000_0100;
const IN_DELETE: u32 = 0x0000_0200;
const IN_ISDIR: u32 = 0x4000_0000;

fn temp_dir() -> PathBuf {
    let mut dir = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    dir.push(format!("inotify-test-{nanos}"));
    std::fs::create_dir(&dir).expect("failed to create temp dir");
    dir
}

fn wait_for_event<F>(mut poll: F, predicate: impl Fn(&Event) -> bool) -> bool
where
    F: FnMut() -> std::io::Result<Vec<Event>>,
{
    for _ in 0..50 {
        let events = poll().expect("failed to read inotify events");
        if events.iter().any(|event| predicate(event)) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn inotify_reports_file_create_and_delete() {
    let root = temp_dir();
    let file_path = root.join("file.bin");

    let mut inotify = Inotify::new().expect("failed to create inotify");
    inotify
        .add_watch(&root, IN_CREATE | IN_DELETE | IN_ISDIR)
        .expect("failed to add watch");

    std::fs::write(&file_path, b"hello").expect("failed to create file");
    assert!(
        wait_for_event(
            || inotify.read_events(),
            |event| (event.mask & IN_CREATE) != 0
        ),
        "expected to observe file creation event",
    );

    std::fs::remove_file(&file_path).expect("failed to remove file");
    assert!(
        wait_for_event(
            || inotify.read_events(),
            |event| (event.mask & IN_DELETE) != 0
        ),
        "expected to observe file deletion event",
    );

    std::fs::remove_dir_all(&root).expect("failed to cleanup temp dir");
}

#[test]
fn inotify_marks_directory_events() {
    let root = temp_dir();
    let nested_dir = root.join("nested");

    let mut inotify = Inotify::new().expect("failed to create inotify");
    inotify
        .add_watch(&root, IN_CREATE | IN_ISDIR)
        .expect("failed to add watch");

    std::fs::create_dir(&nested_dir).expect("failed to create nested dir");
    assert!(
        wait_for_event(
            || inotify.read_events(),
            |event| ((event.mask & IN_CREATE) != 0) && ((event.mask & IN_ISDIR) != 0),
        ),
        "expected to observe directory creation event",
    );

    std::fs::remove_dir_all(&root).expect("failed to cleanup temp dir");
}
