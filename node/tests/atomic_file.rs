use std::fs;
use std::path::PathBuf;
use std::thread;

use rand::Rng;
use the_block::util::atomic_file::write_atomic;

#[test]
fn crash_simulated_write_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.bin");
    let old = b"old".to_vec();
    write_atomic(&path, &old).unwrap();

    let mut rng = rand::thread_rng();
    let new: Vec<u8> = (0..128).map(|_| rng.gen()).collect();
    let path_clone = path.clone();
    let new_clone = new.clone();
    let handle = thread::spawn(move || {
        let _ = write_atomic(&path_clone, &new_clone);
    });

    let mut tmp_os = path.as_os_str().to_owned();
    tmp_os.push(".tmp");
    let tmp_path = PathBuf::from(tmp_os);
    while !tmp_path.exists() {
        thread::yield_now();
    }
    let _ = fs::rename(&tmp_path, &path);
    handle.join().unwrap();

    let final_bytes = fs::read(&path).unwrap();
    assert!(final_bytes == old || final_bytes == new);
}

#[test]
fn concurrent_writers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("race.bin");
    let mut rng = rand::thread_rng();
    let a: Vec<u8> = (0..256).map(|_| rng.gen()).collect();
    let b: Vec<u8> = (0..256).map(|_| rng.gen()).collect();

    let path1 = path.clone();
    let a_clone = a.clone();
    let t1 = thread::spawn(move || write_atomic(&path1, &a_clone).unwrap());
    let path2 = path.clone();
    let b_clone = b.clone();
    let t2 = thread::spawn(move || write_atomic(&path2, &b_clone).unwrap());
    t1.join().unwrap();
    t2.join().unwrap();

    let final_bytes = fs::read(&path).unwrap();
    assert!(final_bytes == a || final_bytes == b);
}
