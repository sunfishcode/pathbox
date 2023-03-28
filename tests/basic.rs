use preopener::{MagicLevel, Preopener};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};

#[test]
fn copy() {
    let dir = tempfile::tempdir().unwrap();
    let real_input_name = dir.path().join("input.txt");
    let real_output_name = dir.path().join("output.txt");

    let mut setup = fs::File::create(&real_input_name).unwrap();
    setup.write_all(b"some data\n").unwrap();

    let args = [
        OsString::from(real_input_name.clone()),
        OsString::from(real_output_name.clone()),
    ];
    let mut preopener = Preopener::new(MagicLevel::Auto);
    let args = preopener.process_args_os(args.into_iter()).unwrap();

    let input_name = &args[0];
    let output_name = &args[1];

    // The real names are hidden.
    assert!(!input_name.contains(real_input_name.to_str().unwrap()));
    assert!(!output_name.contains(real_output_name.to_str().unwrap()));

    let mut input = preopener.open(input_name).unwrap();
    let mut output = preopener.create(output_name).unwrap();

    io::copy(&mut input, &mut output).unwrap();

    // Check that the copy succeeded.
    let mut check = fs::File::open(real_output_name).unwrap();
    let mut contents = Vec::new();
    check.read_to_end(&mut contents).unwrap();
    assert_eq!(contents, b"some data\n");
}

// Like `copy` but doesn't use magic.
#[test]
fn copy_fail_no_magic() {
    let dir = tempfile::tempdir().unwrap();
    let real_input_name = dir.path().join("input.txt");
    let real_output_name = dir.path().join("output.txt");

    let mut setup = fs::File::create(&real_input_name).unwrap();
    setup.write_all(b"some data\n").unwrap();

    let args = [
        OsString::from(real_input_name.clone()),
        OsString::from(real_output_name.clone()),
    ];
    let mut preopener = Preopener::new(MagicLevel::None);
    let args = preopener.process_args_os(args.into_iter()).unwrap();

    let input_name = &args[0];
    let output_name = &args[1];

    // The real names are not hidden.
    assert!(input_name.contains(real_input_name.to_str().unwrap()));
    assert!(output_name.contains(real_output_name.to_str().unwrap()));

    // Everything fails.
    assert_eq!(
        preopener.open(input_name).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        preopener.create(output_name).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

// Like `copy` but doesn't use enough magic.
#[test]
fn copy_fail_no_writeability() {
    let dir = tempfile::tempdir().unwrap();
    let real_input_name = dir.path().join("input.txt");
    let real_output_name = dir.path().join("output.txt");

    let mut setup = fs::File::create(&real_input_name).unwrap();
    setup.write_all(b"some data\n").unwrap();

    let args = [
        OsString::from(real_input_name.clone()),
        OsString::from(real_output_name.clone()),
    ];
    let mut preopener = Preopener::new(MagicLevel::Readonly);
    let args = preopener.process_args_os(args.into_iter()).unwrap();

    let input_name = &args[0];
    let output_name = &args[1];

    // The real names are hidden.
    assert!(!input_name.contains(real_input_name.to_str().unwrap()));
    assert!(!output_name.contains(real_output_name.to_str().unwrap()));

    // In readonly mode we can open the input but opening the output fails.
    let _input = preopener.open(input_name).unwrap();
    assert_eq!(
        preopener.create(output_name).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}
