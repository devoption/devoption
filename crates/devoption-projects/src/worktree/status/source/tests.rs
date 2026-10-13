use super::*;

#[test]
fn protocol_rejects_missing_malformed_mismatched_and_oversized_objects() {
    let id = "a".repeat(40);
    let requests = vec![(id.clone(), "commit")];
    for row in [
        format!("{id} missing\n"),
        format!("{id} commit -1\n"),
        format!("{id} commit +1\n"),
        format!("{id} tag 1\n"),
        format!("{id} blob 1\n"),
        format!("{} commit 1\n", "b".repeat(40)),
        format!("{id} commit {}\n", MAX_OUTPUT + 1),
        format!("{id} commit 1\nextra\n"),
    ] {
        assert!(batch_headers(row.as_bytes(), &requests).is_err());
    }
    let rows = format!("{id} commit 1\n");
    let headers = batch_headers(rows.as_bytes(), &requests).unwrap();
    for payload in [
        format!("{id} commit 2\nab\n"),
        format!("{id} commit 1\na"),
        format!("{id} commit 1\na\nextra"),
        format!("{id} commit 1\n"),
        format!("{id} commit 1\naX"),
    ] {
        assert!(batch_contents(payload.as_bytes(), &headers).is_err());
    }
    assert_eq!(
        batch_contents(format!("{id} commit 1\na\n").as_bytes(), &headers)
            .unwrap()
            .commit(),
        b"a"
    );
    for listing in [
        format!("120000 blob {id}\tlink\0"),
        format!("100644 blob {id}\t../path\0"),
        format!("100644 blob {id}\t.git/config\0"),
        format!("100644 tree {id}\tfile\0"),
        format!("100644 blob {id}\tfile"),
        format!("100644 blob {id}\tfile\0").repeat(MAX_OBJECTS + 1),
    ] {
        assert!(tree_ids(listing.as_bytes()).is_err());
    }
}

#[test]
fn bounded_process_accepts_fixed_file_input_and_limits_output() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("input");
    fs::write(&path, b"fixed input\n").unwrap();
    let run_cat = |limit| {
        run_bounded(
            &mut Command::new("/bin/cat"),
            DEADLINE,
            limit,
            File::open(&path).unwrap().into(),
        )
    };
    assert_eq!(run_cat(12).unwrap(), b"fixed input\n");
    assert_eq!(run_cat(4), Err(Error::LimitExceeded));
}
