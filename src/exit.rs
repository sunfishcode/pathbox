pub enum Status {
    Success,
    Failure,
}

pub fn exit(status: Status) -> ! {
    match status {
        Status::Success => std::process::exit(0),
        Status::Failure => std::process::exit(1),
    }
}
