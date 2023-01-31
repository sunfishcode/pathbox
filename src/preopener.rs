use crate::{log, Level, Writer};
use dir_view::{ambient_authority, DirView, ViewKind};
#[cfg(unix)]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io;

/// The level of path inference that should be performed.
///
/// The `Preopener` facility is capable of recognizing a variety of path
/// strings automatically, which can be very convenient for basic command-line
/// usage.
///
/// The automatic mode does include some protections against common hazards,
/// but can't get everything right. It may fail to recognize a path, causing
/// errors, or it may misidentify a path, potentially granting unintended
/// access to a file or directory.
///
/// This enum allows users to select which level of convenience vs.
/// explicitness they wish to use.
#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub enum MagicLevel {
    /// No magic. No preopens are produced and no strings are translated.
    None,

    /// Interpret `%`-prefixed special arguments, but do nothing else.
    Escapes,

    /// Interpret `%`-prefixed special arguments, and also infer read-only
    /// access for path-like strings.
    Readonly,

    /// Interpret `%`-prefixed special arguments, and auto-infer full access
    /// for path-like strings.
    Auto,
}

/// A set of preopens which isolates external paths from internal paths.
pub struct Preopener {
    magic_level: MagicLevel,
    preopens: Vec<Preopen>,
}

impl Preopener {
    /// Construct a new empty instance of `Preopener`.
    pub fn new(magic_level: MagicLevel) -> Self {
        Self {
            magic_level,
            preopens: Vec::new(),
        }
    }

    /// Add the given command-line arguments to the environment, and return a
    /// translated list of arguments.
    pub fn process_args(
        &mut self,
        args: impl Iterator<Item = String>,
    ) -> Result<Vec<String>, Error> {
        let mut new_args = Vec::new();
        for arg in args {
            new_args.push(self.process(arg)?);
        }
        Ok(new_args)
    }

    /// Add the given command-line arguments to the environment, and return a
    /// translated list of arguments.
    pub fn process_args_os(
        &mut self,
        args: impl Iterator<Item = OsString>,
    ) -> Result<Vec<String>, Error> {
        let mut new_args = Vec::new();
        for arg in args {
            new_args.push(self.process_os(arg)?);
        }
        Ok(new_args)
    }

    /// Add the given environment variables the environment, and return a
    /// translated list of environment variables.
    pub fn process_vars(
        &mut self,
        envs: impl Iterator<Item = (String, String)>,
    ) -> Result<Vec<(String, String)>, Error> {
        let mut new_envs = Vec::new();
        for (key, val) in envs {
            new_envs.push((key, self.process(val)?));
        }
        Ok(new_envs)
    }

    /// Add the given environment variables the environment, and return a
    /// translated list of environment variables.
    pub fn process_vars_os(
        &mut self,
        envs: impl Iterator<Item = (OsString, OsString)>,
    ) -> Result<Vec<(String, String)>, Error> {
        let mut new_envs = Vec::new();
        for (key, val) in envs {
            let key = match key.into_string() {
                Ok(key) => key,
                Err(ill) => {
                    return Err(Error(format!(
                        "An environment variable name contains ill-formed Unicode: {:?}",
                        ill
                    )))
                }
            };
            new_envs.push((key, self.process_os(val)?));
        }
        Ok(new_envs)
    }

    /// Open a file given an internal filename.
    pub fn open(&self, path: &str) -> io::Result<File> {
        for preopen in &self.preopens {
            if !matches!(preopen.access, Access::Read | Access::Any) {
                continue;
            }
            if let Some(rest) = path.strip_prefix(&preopen.uuid) {
                let mut path = preopen.original.clone();
                path.push(rest);
                return File::open(path);
            }
        }

        Err(self.preopen_search_failed(path))
    }

    /// Create a file given an internal filename.
    pub fn create(&self, path: &str) -> io::Result<File> {
        for preopen in &self.preopens {
            if !matches!(preopen.access, Access::Write | Access::Any) {
                continue;
            }
            if let Some(rest) = path.strip_prefix(&preopen.uuid) {
                let mut path = preopen.original.clone();
                path.push(rest);
                return File::create(path);
            }
        }

        Err(self.preopen_search_failed(path))
    }

    /// Open a file for appending given an internal filename.
    pub fn append(&self, path: &str) -> io::Result<File> {
        for preopen in &self.preopens {
            if !matches!(preopen.access, Access::Append | Access::Any) {
                continue;
            }
            if let Some(rest) = path.strip_prefix(&preopen.uuid) {
                let mut path = preopen.original.clone();
                path.push(rest);
                return OpenOptions::new().append(true).open(path);
            }
        }

        Err(self.preopen_search_failed(path))
    }

    /// Open a directory given an internal filename.
    pub fn open_dir(&self, path: &str) -> io::Result<DirView> {
        for preopen in &self.preopens {
            if !matches!(
                preopen.access,
                Access::MutableDir | Access::ReadonlyDir | Access::Any
            ) {
                continue;
            }
            if let Some(rest) = path.strip_prefix(&preopen.uuid) {
                let mut path = preopen.original.clone();
                path.push(rest);
                return DirView::open_ambient_dir(path, ViewKind::Readonly, ambient_authority());
            }
        }

        Err(self.preopen_search_failed(path))
    }

    /// Open a mutable directory given an internal filename.
    pub fn open_mutable_dir(&self, path: &str) -> io::Result<DirView> {
        for preopen in &self.preopens {
            if !matches!(preopen.access, Access::MutableDir | Access::Any) {
                continue;
            }
            if let Some(rest) = path.strip_prefix(&preopen.uuid) {
                let mut path = preopen.original.clone();
                path.push(rest);
                return DirView::open_ambient_dir(path, ViewKind::Full, ambient_authority());
            }
        }

        Err(self.preopen_search_failed(path))
    }

    fn preopen_search_failed(&self, path: &str) -> io::Error {
        // Attempt to provide a more detailed error message.
        for preopen in &self.preopens {
            let access = match preopen.access {
                Access::Read => "read",
                Access::Write => "write",
                Access::Append => "append",
                Access::ReadonlyDir => "readonly directory",
                Access::MutableDir => "read/write directory",
                Access::Any => continue,
            };
            if let Some(_rest) = path.strip_prefix(&preopen.uuid) {
                return io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "Preopen '{:?}' only permits {:?} access",
                        preopen.original, access
                    ),
                );
            }
        }

        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "File is not available as a preopen",
        )
    }

    /// Return a standard-output stream which translates any internal filenames
    /// written to it into external filenames.
    pub fn stdout(&self) -> Writer<'_> {
        crate::writer::stdout(self)
    }

    /// Return a standard-error stream which translates any internal filenames
    /// written to it into external filenames.
    pub fn stderr(&self) -> Writer<'_> {
        crate::writer::stderr(self)
    }

    /// Print a log message which translatesa any internal filenames written
    /// to it into external filenames.
    pub fn log(&self, level: Level, context: &str, message: &str) {
        log(&mut self.stderr(), level, context, message)
    }

    /// Replace any paths in `arg` with random UUIDs, and populate `preopens`
    /// with information about the replacements.
    fn process_os(&mut self, arg: OsString) -> Result<String, Error> {
        match arg.into_string() {
            // If it's valid Unicode, apply the normal processing rules.
            Ok(s) => self.process(s),

            // Interpret any ill-formed string as a filename path, because
            // why else would there be an ill-formed command-line or
            // environment variable string?
            #[cfg(unix)]
            Err(s) => {
                let default_access = match self.magic_level {
                    MagicLevel::Auto => Access::Any,
                    MagicLevel::Readonly => Access::Read,
                    MagicLevel::Escapes | MagicLevel::None => {
                        return Err(Error(
                            "ill-formed strings require a greater magic level".to_owned(),
                        ))
                    }
                };
                Ok(self.replace_os_with_uuid(&s, default_access))
            }

            #[cfg(not(unix))]
            Err(_) => Err(Error("ill-formed strings are not permitted".to_owned())),
        }
    }

    /// Replace any paths in `arg` with random UUIDs, and populate `preopens`
    /// with information about the replacements.
    fn process(&mut self, arg: String) -> Result<String, Error> {
        // Leading '%' is an escape to allow for special features.
        if self.magic_level >= MagicLevel::Escapes {
            if let Some(rest) = arg.strip_prefix('%') {
                // `%verbatim:` means the remainder is a verbatim string.
                if let Some(verbatim) = rest.strip_prefix("verbatim:") {
                    return Ok(verbatim.to_owned());
                }
                // `%read:` means the remainder is a file that may be opened for reading.
                if let Some(path) = rest.strip_prefix("read:") {
                    return Ok(self.replace_with_uuid(path, Access::Read));
                }
                // `%write:` means the remainder is a file that may be opened for writing,
                // creating, and truncating.
                if let Some(path) = rest.strip_prefix("write:") {
                    return Ok(self.replace_with_uuid(path, Access::Write));
                }
                // `%append:` means the remainder is a file that may be opened for appending.
                if let Some(path) = rest.strip_prefix("append:") {
                    return Ok(self.replace_with_uuid(path, Access::Append));
                }
                // `%dir:` means the remainder is a read-only directory.
                if let Some(path) = rest.strip_prefix("dir:") {
                    return Ok(self.replace_with_uuid(path, Access::ReadonlyDir));
                }
                // `%mutable-dir:` means the remainder is a mutable directory.
                if let Some(path) = rest.strip_prefix("mutable-dir:") {
                    return Ok(self.replace_with_uuid(path, Access::MutableDir));
                }

                return Err(Error("Arguments beginning with '%' have special meanings. Prepend \"%verbatim:\" to pass a verbatim argument through.".to_owned()));
            }

            if self.magic_level >= MagicLevel::Readonly {
                let default_access = if self.magic_level >= MagicLevel::Auto {
                    Access::Any
                } else {
                    Access::Read
                };

                if arg.contains(':') {
                    // If all the parts between ':'s look like paths, interpret the
                    // argument as a colon-separated list of paths.
                    if arg.split(':').all(is_likely_path) {
                        return Ok(arg
                            .split(':')
                            .map(|part| self.replace_with_uuid(part, default_access))
                            .collect::<Vec<_>>()
                            .join(":"));
                    }

                    return Ok(arg.to_owned());
                }

                if let Some(eq) = arg.find('=') {
                    let (prefix, suffix) = arg.split_at(eq + 1);
                    if !prefix.contains('/') && is_likely_path(suffix) {
                        // No slash before the '=' and a slash after; treat it as
                        // a `--input=/path/to/file.txt` case and replace the path part.
                        return Ok(
                            prefix.to_owned() + &self.replace_with_uuid(suffix, default_access)
                        );
                    }
                }

                if is_likely_path(&arg) {
                    return Ok(self.replace_with_uuid(&arg, default_access));
                }
            }
        }

        Ok(arg.to_owned())
    }

    fn replace_with_uuid(&mut self, s: &str, access: Access) -> String {
        let (base, ext) = split_extension(s);

        let uuid = format!("wasi-preopen.{}", uuid::Uuid::new_v4());
        self.preopens.push(Preopen {
            uuid: uuid.clone(),
            original: base.to_owned().into(),
            access,
        });
        uuid + ext
    }

    #[cfg(unix)]
    fn replace_os_with_uuid(&mut self, s: &OsStr, access: Access) -> String {
        let uuid = format!("wasi-preopen.{}", uuid::Uuid::new_v4());
        self.preopens.push(Preopen {
            uuid: uuid.clone(),
            original: s.to_owned(),
            access,
        });
        uuid
    }

    pub(crate) fn as_slice(&self) -> &[Preopen] {
        &self.preopens
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Error(String);

impl std::error::Error for Error {}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        std::fmt::Display::fmt(self, f)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        self.0.fmt(f)
    }
}

/// A record of a name which has been replaced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Preopen {
    /// The replacement string.
    pub(crate) uuid: String,

    /// The original string.
    pub(crate) original: OsString,

    /// How the file may be accessed.
    pub(crate) access: Access,
}

/// What types of file access should be permitted?
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Access {
    /// Allow read-only operations.
    Read,
    /// Allow writing, creating, and truncating files.
    Write,
    /// Allow appending to files.
    Append,
    /// Allow read-only directory operations.
    ReadonlyDir,
    /// Allow all directory operations.
    MutableDir,
    /// Allow any access to files.
    Any,
}

/// If `s` is a path ending with a basename extension, split it into the
/// path without the extension, and the extension.
fn split_extension(s: &str) -> (&str, &str) {
    let after_last_slash = match s.rfind('/') {
        Some(slash) => slash + 1,
        None => 0,
    };
    let basename = &s[after_last_slash..];

    // If there are no embedded '.'s in the basename, there's no extension.
    if !basename.contains('.') {
        return (s, "");
    }

    // The extension is the substring after the first dot which is not:
    //  - at the beginning of the basename,
    //  - immediately followed by another dot, or
    //  - followed by a USV which is never an extension USV.
    let mut remainder = &basename[1..];
    while let Some(last_dot) = remainder.find('.') {
        let ext = &remainder[last_dot + 1..];
        if !ext.starts_with('.') && !ext.chars().any(is_never_extension) {
            let suffix = &remainder[last_dot..];
            return (&s[..s.len() - suffix.len()], suffix);
        }
        remainder = ext;
    }

    // Otherwise, there is no extension.
    (s, "")
}

/// Test whether `c` is a `char` which is never part of a filename extension.
fn is_never_extension(c: char) -> bool {
    // These should be excluded already.
    assert_ne!(c, '/');

    // The following characters should never be permitted as a filename
    // extension.
    c.is_whitespace() || c.is_control()
        // Selected ASCII codes which would be trouble an extension and as such
        // are very unlikely to be an extension in the wild.
        || c == '*'
        || c == '"'
        || c == '\''
        || c == '`'
        || c == ':'
        || c == ';'
        || c == '\\'
        || c == '('
        || c == ')'
        || c == '{'
        || c == '}'
        || c == '['
        || c == ']'
        || c == '|'
        || c == '>'
        || c == '<'
        // Just... no.
        || c == '\u{feff}' // BOM
        // Unicode specials are special.
        || c == '\u{fff9}' // Interlinear annotations
        || c == '\u{fffa}'
        || c == '\u{fffb}'
        || c == '\u{fffc}' // Object-replacement character
        || c == char::REPLACEMENT_CHARACTER
}

/// Test whether `c` is a suspicious shell metacharacter which is unlikely to be
/// worth assuming participates in a filename.
fn is_suspicious_shell_metacharacter(c: char) -> bool {
    // On Windows, backslash is a path separator.
    #[cfg(windows)]
    {
        if c == '\\' {
            return false;
        }
    }

    matches!(
        c,
        '&' | '<' | '>' | '\\' | '|' | '?' | '*' | '[' | ']' | '"' | '\'' | ';'
    )
}

/// Apply some simple heuristics to determine whether `arg` is likely to refer
/// to a filesystem path.
///
/// The heuristic roughly works like this:
///
///  - If it starts with a `-`, assume it's not a path.
///  - If it contains a `/`, assume it is a path.
///  - If it ends with a conventional-looking filename extension, or it looks like
///    a dotile, assume it is a path.
///  - Otherwise, assume it isn't.
///
/// There are also a few additional heuristics for rare situations.
fn is_likely_path(arg: &str) -> bool {
    // Exceptionally long strings are never filesystem paths.
    if arg.len() > 4096 {
        return false;
    }

    if let Some(c) = arg.chars().next() {
        // If the name starts with '-', assume it's meant to be a flag.
        if c == '-' {
            return false;
        }

        // On Windows, also assume a leading slash is meant to be a flag.
        #[cfg(windows)]
        if c == '/' {
            return false;
        }

        // If the name has leading whitespace, assume it's not a path.
        if c.is_whitespace() {
            return false;
        }

        // If the name starts with suspicious shell beginning-of-string
        // metacharacters, don't give it the benefit of the doubt.
        if matches!(c, '~' | '!') {
            return false;
        }

        // We use a leading `%` as our escape character.
        if c == '%' {
            return false;
        }

        // If the name starts with suspicious shell metacharacters, don't give
        // it the benefit of the doubt.
        if is_suspicious_shell_metacharacter(c) {
            return false;
        }
    } else {
        // Empty strings are never filesystem paths.
        return false;
    }

    // If any path-looking component begins or ends with whitespace, or ends
    // with a `.` (without being `.` or `..` themselves) then assume it's not
    // a path.
    for component in std::path::Path::new(arg).components() {
        let component = component.as_os_str().to_str().unwrap();

        // `.` and `..` are common path components.
        if component == "." || component == ".." {
            continue;
        }

        if let Some(first) = component.chars().next() {
            if first.is_whitespace() {
                return false;
            }
        } else {
            return false;
        }
        let last = component.chars().rev().next().unwrap();
        if last.is_whitespace() {
            return false;
        }
        if last == '.' {
            return false;
        }
    }

    // Filenames containing control characters aren't impossible, but are very
    // rare and more likely to indicate something amiss than something normal.
    if arg.chars().any(char::is_control) {
        return false;
    }

    // Now that we've ruled out patterns that are very likely to indicate that
    // something is not meant to be a path, check for patterns which indicate
    // that is likely to indicate that it is meant to be a path.

    // If it contains a `/`, treat it as a path.
    if arg.contains('/') {
        return true;
    }

    // Recognize Windows' special filenames as paths.
    #[cfg(windows)]
    {
        let (start, _ext) = split_extension(arg);
        for special in [
            "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
            "COM7", "COM8", "COM9", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
            "LPT8", "LPT9",
        ] {
            if start.eq_ignore_ascii_case(special) {
                true;
            }
        }
    }

    // On Windows, if it starts with a path prefix, treat it as a path.
    #[cfg(windows)]
    if matches!(
        std::path::Path::new(arg).components().next(),
        Some(std::path::Component::Prefix(_))
    ) {
        return true;
    }

    // On Windows, if it contains `\\`, treat it as a path.
    #[cfg(windows)]
    if arg.contains('\\') {
        return true;
    }

    // Recognize plain filenames if they have a conventional-looking
    // filename extension.
    if let Some(ext) = std::path::Path::new(arg).extension() {
        if let Some(ext) = ext.to_str() {
            if !ext.is_empty() && ext.len() <= 16 && ext.chars().all(|c| c.is_ascii_alphanumeric())
            {
                return true;
            }
        }
    }

    // Similarly, recognize Unix-style dot files.
    if let Some(suffix) = arg.strip_prefix('.') {
        if suffix.chars().all(|c| {
            c.is_ascii_graphic()
                && !matches!(
                    c,
                    '&' | '<' | '>' | '\\' | '|' | '?' | '*' | '[' | ']' | '"' | '\'' | ';'
                )
        }) {
            return true;
        }
    }

    false
}

#[test]
fn test_is_likely_path() {
    assert!(is_likely_path("/"));
    assert!(is_likely_path("//"));
    assert!(is_likely_path("."));
    assert!(is_likely_path(".."));
    assert!(is_likely_path("/."));
    assert!(is_likely_path("/.."));
    assert!(is_likely_path("./"));
    assert!(is_likely_path("../"));
    assert!(is_likely_path("hello.mp3"));
    assert!(is_likely_path("world.JPEG"));
    assert!(is_likely_path("goodnight.d"));
    assert!(is_likely_path("moon.delightful"));
    assert!(is_likely_path(".gitignore"));
    assert!(is_likely_path(".this-and_that"));
    assert!(is_likely_path("/foo"));
    assert!(is_likely_path("/foo/bar"));
    assert!(is_likely_path("/foo.baz/bar.baz"));
    assert!(is_likely_path("foo/bar"));
    assert!(is_likely_path("foo.baz/bar.baz"));
    assert!(is_likely_path("foo/"));
    assert!(is_likely_path("foo/bar/"));
    assert!(is_likely_path("foo/bar/."));
    assert!(is_likely_path("fo o/b ar"));
    assert!(is_likely_path("f oo/ba r"));
    assert!(is_likely_path(&"A/".repeat(2048)));

    assert!(!is_likely_path(""));
    assert!(!is_likely_path(".this and that"));
    assert!(!is_likely_path("/hello\nworld.txt"));
    assert!(!is_likely_path("/hello\tworld.txt"));
    assert!(!is_likely_path("/hello\0world.txt"));
    assert!(!is_likely_path(".this and that"));
    assert!(!is_likely_path("<special/time.txt"));
    assert!(!is_likely_path("!/what.txt"));
    assert!(!is_likely_path("*/*/foo.md"));
    assert!(!is_likely_path("foo"));
    assert!(!is_likely_path(" /foo"));
    assert!(!is_likely_path("foo /bar"));
    assert!(!is_likely_path("foo/ bar"));
    assert!(!is_likely_path("foo/bar."));
    assert!(!is_likely_path("foo./bar"));
    assert!(!is_likely_path("moon.excessivelylongextension"));
    assert!(!is_likely_path(&"A/".repeat(2049)));

    assert_eq!(is_likely_path("foo\\bar"), cfg!(windows));
    assert_eq!(is_likely_path("\\foo\\bar"), cfg!(windows));
    assert_eq!(is_likely_path("/A"), !cfg!(windows));
    assert_eq!(is_likely_path("CON"), cfg!(windows));
    assert_eq!(is_likely_path("NUL"), cfg!(windows));
    assert_eq!(is_likely_path(r"\\?\pictures\kittens"), cfg!(windows));
    assert_eq!(is_likely_path(r"\\?\UNC\server\share"), cfg!(windows));
    assert_eq!(is_likely_path(r"\\?\c:\"), cfg!(windows));
    assert_eq!(is_likely_path(r"\\.\BrainInterface"), cfg!(windows));
    assert_eq!(is_likely_path(r"\\server\share"), cfg!(windows));
    assert_eq!(
        is_likely_path(r"C:\Users\Rust\Pictures\Ferris"),
        cfg!(windows)
    );
}

#[test]
fn test_split_extension() {
    assert_eq!(split_extension("/foo/bar"), ("/foo/bar", ""));
    assert_eq!(split_extension("/foo/bar.txt"), ("/foo/bar", ".txt"));
    assert_eq!(
        split_extension("/foo.qux/bar.txt"),
        ("/foo.qux/bar", ".txt")
    );
    assert_eq!(split_extension("/foo/.bar"), ("/foo/.bar", ""));
    assert_eq!(split_extension("/foo/.bar.txt"), ("/foo/.bar", ".txt"));
    assert_eq!(
        split_extension("/foo.qux/.bar.txt"),
        ("/foo.qux/.bar", ".txt")
    );
    assert_eq!(
        split_extension("/foo/.bar.txt.gz"),
        ("/foo/.bar", ".txt.gz")
    );
    assert_eq!(
        split_extension("/foo.qux/.bar.txt.gz"),
        ("/foo.qux/.bar", ".txt.gz")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.txt"),
        ("/foo.qux/bar", ".txt")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.txt.gz"),
        ("/foo.qux/bar", ".txt.gz")
    );
    assert_eq!(
        split_extension("/foo.qux/bar..txt.gz"),
        ("/foo.qux/bar.", ".txt.gz")
    );
    assert_eq!(split_extension("/foo.qux/bar.*"), ("/foo.qux/bar.*", ""));
    assert_eq!(
        split_extension("/foo.qux/bar.*.txt"),
        ("/foo.qux/bar.*", ".txt")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.*.txt.gz"),
        ("/foo.qux/bar.*", ".txt.gz")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffd}"),
        ("/foo.qux/bar.\u{fffd}", "")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffd}.txt"),
        ("/foo.qux/bar.\u{fffd}", ".txt")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffd}.txt.gz"),
        ("/foo.qux/bar.\u{fffd}", ".txt.gz")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{feff}"),
        ("/foo.qux/bar.\u{feff}", "")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{feff}.txt"),
        ("/foo.qux/bar.\u{feff}", ".txt")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{feff}.txt.gz"),
        ("/foo.qux/bar.\u{feff}", ".txt.gz")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffa}"),
        ("/foo.qux/bar.\u{fffa}", "")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffa}.txt"),
        ("/foo.qux/bar.\u{fffa}", ".txt")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffa}.txt.gz"),
        ("/foo.qux/bar.\u{fffa}", ".txt.gz")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffb}"),
        ("/foo.qux/bar.\u{fffb}", "")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffb}.txt"),
        ("/foo.qux/bar.\u{fffb}", ".txt")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffb}.txt.gz"),
        ("/foo.qux/bar.\u{fffb}", ".txt.gz")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffc}"),
        ("/foo.qux/bar.\u{fffc}", "")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffc}.txt"),
        ("/foo.qux/bar.\u{fffc}", ".txt")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{fffc}.txt.gz"),
        ("/foo.qux/bar.\u{fffc}", ".txt.gz")
    );
    assert_eq!(split_extension("/foo.qux/bar. "), ("/foo.qux/bar. ", ""));
    assert_eq!(
        split_extension("/foo.qux/bar. .txt"),
        ("/foo.qux/bar. ", ".txt")
    );
    assert_eq!(
        split_extension("/foo.qux/bar. .txt.gz"),
        ("/foo.qux/bar. ", ".txt.gz")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{7}"),
        ("/foo.qux/bar.\u{7}", "")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{7}.txt"),
        ("/foo.qux/bar.\u{7}", ".txt")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{7}.txt.gz"),
        ("/foo.qux/bar.\u{7}", ".txt.gz")
    );
    assert_eq!(
        split_extension("/foo.qux/bar.\u{7}. .*..txt.gz"),
        ("/foo.qux/bar.\u{7}. .*.", ".txt.gz")
    );
    assert_eq!(split_extension("."), (".", ""));
    assert_eq!(split_extension(".txt"), (".txt", ""));
    assert_eq!(split_extension("a.txt"), ("a", ".txt"));
    assert_eq!(split_extension("a..txt"), ("a.", ".txt"));
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Eq, PartialEq, Debug)]
    struct Process {
        arg: String,
        preopens: Vec<Preopen>,
    }

    impl Process {
        fn new(arg: &str, preopens: &[Preopen]) -> Self {
            Self {
                arg: arg.to_owned(),
                preopens: preopens.to_vec(),
            }
        }
    }

    fn do_process(arg: &str) -> Result<Process, Error> {
        let mut preopener = Preopener::new(MagicLevel::Auto);
        let arg = preopener.process_args([arg.to_owned()].into_iter())?;
        Ok(Process {
            arg: arg[0].clone(),
            preopens: preopener.as_slice().to_vec(),
        })
    }

    fn do_process_os(arg: &OsStr) -> Result<Process, Error> {
        let mut preopener = Preopener::new(MagicLevel::Auto);
        let arg = preopener.process_args_os([arg.to_owned()].into_iter())?;
        Ok(Process {
            arg: arg[0].clone(),
            preopens: preopener.as_slice().to_vec(),
        })
    }

    #[test]
    fn test_error() {
        let args = ["%", "%%", "%%%", "%/foo"];
        for arg in args {
            assert!(do_process(arg).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_invalid() {
        use std::os::unix::ffi::OsStrExt;

        let args = [
            &b"hi/\xc0"[..],
            &b"hello/\xff"[..],
            &b"greetings/hello\x80world"[..],
        ];
        for arg in args {
            let p = do_process_os(OsStr::from_bytes(arg)).unwrap();
            assert_eq!(p.preopens.len(), 1);
            assert_eq!(p.arg, p.preopens[0].uuid);
            assert_eq!(p.preopens[0].original, OsStr::from_bytes(arg));
        }
    }

    #[test]
    fn test_passthrough() {
        let args = [
            "",
            "foo",
            "--input=foo",
            "foo%",
            "0123:4567:89ab:cdef:0246:8ace:1357:9bdf",
            "::1",
            "username@hostname:",
            "username@hostname:foo",
            "hostname:",
            "hostname:foo",
            "hostname:80",
            "hostname:/tmp",
            "username@hostname:/tmp",
            "https://example.com",
            "https://example.com:80",
            "https://example.com/",
            "https://example.com:80/",
            "data:,Hello%2C%20World!",
            "data:text/plain;base64,SGVsbG8sIFdvcmxkIQ==",
            "--input=foo:bar",
            "--input=foo:/bar",
            "--input=/foo:bar",
            ":",
            "::",
            "[:alnum:]",
        ];
        for arg in args {
            assert_eq!(do_process(arg), Ok(Process::new(arg, &[])));
            assert_eq!(
                do_process(&("%verbatim:".to_owned() + arg)),
                Ok(Process::new(arg, &[]))
            );
        }
    }

    #[test]
    fn test_verbatim() {
        let args = [
            "/some/arg",
            "/",
            "colon/separated:list/of:args/here",
            "/:/",
            "--input=/some/arg",
            "--input=/",
        ];
        for arg in args {
            assert_eq!(
                do_process(&("%verbatim:".to_owned() + arg)),
                Ok(Process::new(arg, &[]))
            );

            let result = do_process(arg).unwrap();
            assert_ne!(arg, result.arg);
            assert!(!result.preopens.is_empty());
        }
    }

    #[test]
    fn test_paths() {
        let args = [
            "/",
            "./",
            "//",
            ".//",
            ".foo",
            "foo/bar",
            "foo/bar/",
            "foo/bar/",
            "foo/bar/.",
            "/foo",
            "/foo/bar",
            "/foo/bar/",
            "/foo/bar/.",
            "//foo",
            "//foo/bar",
            "//foo/bar/",
            "//foo/bar/.",
        ];
        for arg in args {
            let p = do_process(arg).unwrap();
            assert_eq!(p.preopens.len(), 1);
            assert_eq!(p.arg, p.preopens[0].uuid);
            assert_eq!(p.preopens[0].original, arg);
            assert_eq!(
                do_process(&("%verbatim:".to_owned() + arg)),
                Ok(Process::new(arg, &[]))
            );
        }
    }

    #[test]
    fn test_colon_separated() {
        let p = do_process("/:/").unwrap();
        assert_eq!(p.preopens.len(), 2);
        assert_eq!(
            p.arg,
            format!("{}:{}", p.preopens[0].uuid, p.preopens[1].uuid)
        );
        assert_eq!(p.preopens[0].original, "/");
        assert_eq!(p.preopens[1].original, "/");
        assert_eq!(
            do_process(&("%verbatim:/:/".to_owned())),
            Ok(Process::new("/:/", &[]))
        );

        let p = do_process("./foo:./bar").unwrap();
        assert_eq!(p.preopens.len(), 2);
        assert_eq!(
            p.arg,
            format!("{}:{}", p.preopens[0].uuid, p.preopens[1].uuid)
        );
        assert_eq!(p.preopens[0].original, "./foo");
        assert_eq!(p.preopens[1].original, "./bar");
        assert_eq!(
            do_process(&("%verbatim:./foo:./bar".to_owned())),
            Ok(Process::new("./foo:./bar", &[]))
        );
    }

    #[test]
    fn test_more_colon_separated() {
        let name = "name:with:colons";
        assert_eq!(do_process(name), Ok(Process::new("name:with:colons", &[])));

        let name = "/name:%with/:col/ons";
        assert_eq!(
            do_process(name),
            Ok(Process::new("/name:%with/:col/ons", &[]))
        );

        let name = "/name:with/:col/ons";
        let p = do_process(name).unwrap();
        assert_eq!(
            p.arg,
            format!(
                "{}:{}:{}",
                p.preopens[0].uuid, p.preopens[1].uuid, p.preopens[2].uuid
            )
        );
    }

    #[test]
    fn test_equals() {
        let p = do_process("--input=/foo").unwrap();
        assert!(p.arg.starts_with("--input="));
        assert!(!p.arg.ends_with("/foo"));
        assert_eq!(p.preopens.len(), 1);
        assert_eq!(p.arg, format!("--input={}", p.preopens[0].uuid));
        assert_eq!(p.preopens[0].original, "/foo");
        assert_eq!(
            do_process("%verbatim:--input=/foo"),
            Ok(Process::new("--input=/foo", &[]))
        );
    }
}
