use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use preopener::{MagicLevel, Preopens};

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
    let mut preopens = Preopens::new(MagicLevel::Auto);
    let args = preopens.process_args_os(args.into_iter()).unwrap();

    let input_name = &args[0];
    let output_name = &args[1];

    // The real names are hidden.
    assert!(!input_name.contains(&real_input_name.to_str().unwrap()));
    assert!(!output_name.contains(&real_output_name.to_str().unwrap()));

    let mut input = preopens.open(input_name).unwrap();
    let mut output = preopens.create(output_name).unwrap();

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
    let mut preopens = Preopens::new(MagicLevel::None);
    let args = preopens.process_args_os(args.into_iter()).unwrap();

    let input_name = &args[0];
    let output_name = &args[1];

    // The real names are not hidden.
    assert!(input_name.contains(&real_input_name.to_str().unwrap()));
    assert!(output_name.contains(&real_output_name.to_str().unwrap()));

    // Everything fails.
    assert_eq!(
        preopens.open(input_name).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        preopens.create(output_name).unwrap_err().kind(),
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
    let mut preopens = Preopens::new(MagicLevel::Readonly);
    let args = preopens.process_args_os(args.into_iter()).unwrap();

    let input_name = &args[0];
    let output_name = &args[1];

    // The real names are hidden.
    assert!(!input_name.contains(&real_input_name.to_str().unwrap()));
    assert!(!output_name.contains(&real_output_name.to_str().unwrap()));

    // In readonly mode we can open the input but opening the output fails.
    let _input = preopens.open(input_name).unwrap();
    assert_eq!(
        preopens.create(output_name).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}
