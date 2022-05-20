use crate::Writer;
use std::ffi::{OsStr, OsString};
use std::io;

/// An "environment" which separates external paths from internal paths.
pub struct Env {
    preopens: Vec<Preopen>,
}

impl Env {
    /// Construct a new empty instance of `Env`.
    pub fn new() -> Self {
        Self {
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
    pub fn open(&self, path: &str) -> io::Result<std::fs::File> {
        for preopen in &self.preopens {
            if let Some(rest) = path.strip_prefix(&preopen.uuid) {
                let mut path = preopen.original.clone();
                path.push(rest);
                return std::fs::File::open(path);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "File is not available as a preopen",
        ))
    }

    /// Return a standard-output stream which translates any internal filenames
    /// written to it into external filenames.
    pub fn stdout(&self) -> Writer<'_> {
        crate::writer::stdout(self)
    }

    /// Return a standard-output stream which translates any internal filenames
    /// written to it into external filenames.
    pub fn stderr(&self) -> Writer<'_> {
        crate::writer::stderr(self)
    }

    /// Replace any paths in `arg` with random UUIDs, and populate `preopens`
    /// with information about the replacements.
    fn process_os(&mut self, arg: OsString) -> Result<String, Error> {
        match arg.into_string() {
            Ok(s) => self.process(s),

            // Interpret any ill-formed string as a filename path, because
            // why else would there be an ill-formed command-line or
            // environment variable string?
            #[cfg(unix)]
            Err(s) => Ok(self.replace_os_with_uuid(&s)),

            #[cfg(not(unix))]
            Err(s) => Err("ill-formed strings are not permitted"),
        }
    }

    /// Replace any paths in `arg` with random UUIDs, and populate `preopens`
    /// with information about the replacements.
    fn process(&mut self, arg: String) -> Result<String, Error> {
        // Leading '?' is an escape to allow for special features.
        if let Some(rest) = arg.strip_prefix('?') {
            // `?=` means to pass on the rest of the string verbatim.
            if let Some(verbatim) = rest.strip_prefix('=') {
                return Ok(verbatim.to_owned());
            }

            return Err(Error("Arguments beginning with '?' have special meanings. Prepend \"?=\" to pass a verbatim argument through.".to_owned()));
        }

        if arg.contains(':') {
            // If the string contains "://", interpret it as a URL.
            if arg.contains("://") {
                return Ok(arg.to_owned());
            }

            if is_more_likely_path_than_list(&arg) {
                return Ok(self.replace_with_uuid(&arg));
            }

            // If all the parts between ':'s contain '/'s, interpret the argument
            // as a colon-separated list of paths.
            if arg.split(':').all(is_likely_path) {
                return Ok(arg
                    .split(':')
                    .map(|part| self.replace_with_uuid(part))
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
                return Ok(prefix.to_owned() + &self.replace_with_uuid(suffix));
            }
        }

        if is_likely_path(&arg) {
            return Ok(self.replace_with_uuid(&arg));
        }

        Ok(arg.to_owned())
    }

    fn replace_with_uuid(&mut self, s: &str) -> String {
        let (base, ext) = split_extension(s);

        let uuid = uuid::Uuid::new_v4();
        self.preopens.push(Preopen {
            uuid: uuid.to_string(),
            original: base.to_owned().into(),
        });
        uuid.to_string() + ext
    }

    fn replace_os_with_uuid(&mut self, s: &OsStr) -> String {
        let uuid = uuid::Uuid::new_v4();
        self.preopens.push(Preopen {
            uuid: uuid.to_string(),
            original: s.to_owned(),
        });
        uuid.to_string()
    }

    pub(crate) fn preopens(&self) -> &[Preopen] {
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
}

fn split_extension(s: &str) -> (&str, &str) {
    let last_slash = match s.rfind('/') {
        Some(slash) => slash + 1,
        None => 0,
    };
    let filename = &s[last_slash..];

    // If there are no embedded '.'s in the file name, there's no extension.
    if !filename.contains('.') {
        return (s, "");
    }

    // The extension is the substring after the first dot which is not:
    //  - at the beginning of the filename,
    //  - immediately followed by another dot, or
    //  - followed by a usv which is never an extension usv.
    let mut remainder = &filename[1..];
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

fn is_more_likely_path_than_list(arg: &str) -> bool {
    let colon = arg.find(':').unwrap();
    let s = &arg[colon..];
    let base = s.find('/').unwrap_or(s.len());
    if let Err(e) = std::fs::metadata(&arg[..colon + base]) {
        if e.kind() == io::ErrorKind::NotFound {
            return false;
        }
    }
    true
}

fn is_likely_path(arg: &str) -> bool {
    if arg.contains('/') {
        return true;
    }

    if !mime_guess::from_path(arg).is_empty() {
        match std::fs::metadata(arg) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return false,
            _ => return true,
        }
    }

    false
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

#[test]
fn test_is_more_likely_path_than_list() {
    assert!(!is_more_likely_path_than_list("/does/not/exist:at/all"));
}

#[test]
fn test_colon_path_exists() {
    let dir = tempfile::tempdir().unwrap();

    let name = dir.path().join("name:with:colons");
    std::fs::create_dir(&name).unwrap();
    assert!(is_more_likely_path_than_list(
        camino::Utf8PathBuf::try_from(name).unwrap().as_str()
    ));

    let name = dir.path().join("name:with:colons/and:more/and/more");
    std::fs::create_dir_all(&name).unwrap();
    assert!(is_more_likely_path_than_list(
        camino::Utf8PathBuf::try_from(name).unwrap().as_str()
    ));
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
        let mut env = Env::new();
        let arg = env.process_args([arg.to_owned()].into_iter())?;
        Ok(Process {
            arg: arg[0].clone(),
            preopens: env.preopens().to_vec(),
        })
    }

    #[test]
    fn test_error() {
        let args = ["?", "??", "???", "?/foo"];
        for arg in args {
            assert!(do_process(arg).is_err());
        }
    }

    #[test]
    fn test_passthrough() {
        let args = [
            "",
            "foo",
            "--input=foo",
            ".foo",
            "foo?",
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
                do_process(&("?=".to_owned() + arg)),
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
                do_process(&("?=".to_owned() + arg)),
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
                do_process(&("?=".to_owned() + arg)),
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
            do_process(&("?=/:/".to_owned())),
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
            do_process(&("?=./foo:./bar".to_owned())),
            Ok(Process::new("./foo:./bar", &[]))
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
            do_process("?=--input=/foo"),
            Ok(Process::new("--input=/foo", &[]))
        );
    }
}
