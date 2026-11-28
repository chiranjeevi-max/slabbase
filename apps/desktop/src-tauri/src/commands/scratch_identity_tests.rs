use super::*;
use std::os::unix::fs::symlink;

#[test]
fn publishing_refuses_a_replaced_scratch_path() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let target = directory.path().join("export.csv");
    let (temporary, file) = ExportTemp::create(target.to_str().expect("path")).expect("scratch");
    let scratch = temporary.path.clone();
    drop(file);
    std::fs::remove_file(&scratch).expect("unlink scratch");
    let other = directory.path().join("other");
    std::fs::write(&other, "attacker").expect("other file");
    symlink(&other, &scratch).expect("symlink");
    assert!(
        temporary.publish(target.to_str().expect("path")).is_err(),
        "a replaced scratch path was published"
    );
    assert!(!target.exists());
}
