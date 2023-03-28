use pathbox::{exit, Level, MagicLevel, Pathbox, Status};
use std::io;

fn main() {
    // Set up the arguments and environment variables. This is the part a
    // WASI runtime would do automatically before a program starts.

    let mut args = std::env::args_os();
    args.next(); // skip argv[0]
    let args = args.collect::<Vec<_>>();

    // Print out the arguments before translation.
    eprintln!(">>> external args: {:?}", args);

    let mut pathbox = Pathbox::new(MagicLevel::Auto);
    let args = pathbox.process_args_os(args.into_iter()).unwrap();
    let _vars = pathbox.process_vars_os(std::env::vars_os()).unwrap();

    // Print out the arguments after translation.
    eprintln!(">>> internal args: {:?}", args);

    // Now run the main application.

    let mut args = args.iter();

    // Obtain the string to search for.
    let src = match args.next() {
        Some(what) => what,
        None => {
            eprintln!("usage: cp <input> <output>");
            exit(Status::Failure);
        }
    };
    let dst = match args.next() {
        Some(what) => what,
        None => {
            eprintln!("usage: cp <input> <output>");
            exit(Status::Failure);
        }
    };
    match args.next() {
        None => {}
        Some(_) => {
            eprintln!("usage: cp <input> <output>");
            exit(Status::Failure);
        }
    };

    let mut input = match pathbox.open(src) {
        Ok(f) => f,
        Err(e) => {
            // Use `log` instead of stderr.
            pathbox.log(
                Level::Error,
                "stderr",
                &format!("Error: cannot open file '{}': {}", src, e),
            );
            exit(Status::Failure);
        }
    };
    let mut output = match pathbox.create(dst) {
        Ok(f) => f,
        Err(e) => {
            // Use `log` instead of stderr.
            pathbox.log(
                Level::Error,
                "stderr",
                &format!("Error: cannot open file '{}': {}", dst, e),
            );
            exit(Status::Failure);
        }
    };

    io::copy(&mut input, &mut output).unwrap();

    exit(Status::Success);
}
