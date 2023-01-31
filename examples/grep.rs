use std::io;
use std::io::{BufRead, Write};
use preopener::{exit, Level, MagicLevel, Preopens, Status};

fn main() {
    // Set up the arguments. This is the part a WASI runtime would do
    // automatically before a program starts.

    let mut args = std::env::args_os();
    args.next(); // skip argv[0]
    let args = args.collect::<Vec<_>>();

    // Print out the arguments before translation.
    eprintln!(">>> external args: {:?}", args);

    let mut preopens = Preopens::new(MagicLevel::Auto);
    let args = preopens.process_args_os(args.into_iter()).unwrap();

    // Print out the arguments after translation.
    eprintln!(">>> internal args: {:?}", args);

    // Now run the main application.

    let mut args = args.iter();

    // Obtain the string to search for.
    let what = match args.next() {
        Some(what) => what,
        None => {
            eprintln!("usage: grep pattern paths...");
            exit(Status::Failure);
        }
    };

    // Open the remaining arguments and search for the string.
    for arg in args {
        match preopens.open(&arg) {
            Ok(f) => {
                for line in io::BufReader::new(f).lines() {
                    let line = line.unwrap();
                    // A real grep would use a regex here ¯\_(ツ)_/¯.
                    if line.contains(what) {
                        writeln!(&mut preopens.stdout(), "{}: {}", arg, line).unwrap();
                    }
                }
            }
            Err(e) => {
                // Use `log` instead of stderr.
                preopens.log(
                    Level::Error,
                    "stderr",
                    &format!("Error: cannot open file '{}': {:?}", arg, e),
                );
                exit(Status::Failure);
            }
        }
    }

    exit(Status::Success);
}
