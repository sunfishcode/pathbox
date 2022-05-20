use std::io;
use std::io::{BufRead, Write};
use wasi_env::Env;

fn main() {
    // Set up the arguments and environment variables. This is the part a
    // WASI runtime would do automatically before a program starts.

    let mut args = std::env::args_os();
    args.next(); // skip argv[0]
    let args = args.collect::<Vec<_>>();

    // Print out the arguments before translation.
    eprintln!(">>> external args: {:?}", args);

    let mut env = Env::new();
    let args = env.process_args_os(args.into_iter()).unwrap();
    let _envs = env.process_vars_os(std::env::vars_os()).unwrap();

    // Print out the arguments after translation.
    eprintln!(">>> internal args: {:?}", args);

    // Now run the main application.

    let mut args = args.iter();

    // Obtain the string to search for.
    let what = match args.next() {
        Some(what) => what,
        None => {
            eprintln!("usage: grep pattern paths...");
            std::process::exit(1);
        }
    };

    // Open the remaining arguments and search for the string.
    for arg in args {
        match env.open(&arg) {
            Ok(f) => {
                for line in io::BufReader::new(f).lines() {
                    let line = line.unwrap();
                    // A real grep would use a regex here ¯\_(ツ)_/¯.
                    if line.contains(what) {
                        writeln!(&mut env.stdout(), "{}: {}", arg, line).unwrap();
                    }
                }
            }
            Err(e) => {
                writeln!(
                    &mut env.stderr(),
                    "Error: cannot open file '{}': {:?}",
                    arg,
                    e
                )
                .unwrap();
            }
        }
    }
}
