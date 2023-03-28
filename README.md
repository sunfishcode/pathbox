One of the things we've learned from wasi\_snapshot\_preview1 was that
the `--dir=` preopen system is inconvenient, awkward, and prone to misuse.

And it's especially inconvenient in use cases like binfmt\_misc where users
want to treat it like a "regular" executable where they don't have to pass
extra `--dir=` flags.

This repo contains a possible alternative approach which preserves the
useful properties of preopens while being more convenient.

### Example

examples/grep.rs is a simple grep-like program:

```
$ cargo run --quiet --example grep dep Cargo.toml
>>> external args: ["dep", "Cargo.toml"]
>>> internal args: ["dep", "9b60de2d-fb3f-4cc8-bc9b-0d959f615a0d.toml"]
Cargo.toml: [dependencies]
Cargo.toml: [dev-dependencies]
```

The first two `>>>` lines contain debugging output. They show the arguments
passed to the host process (external args) and the arguments that would
be passed to a WASI program (internal args). The remaining lines are the
normal grep output.

In the internal args, notice that "Cargo.toml" has been replaced by
"9b60de2d-fb3f-4cc8-bc9b-0d959f615a0d.toml". That hides the path
from the application, since paths might contain user-identifying or
host-identifying information. And notice also that the application's stdout
has translated the UUID string back into "Cargo.toml", so that this is
mostly transparent to end users.

This relies on some heuristics to determine which arguments are likely to
refer to filesystem paths. It should work in most cases, however it won't
work in all. For example, a string like "file.silly!" might be a valid
filename, but does not satisfy the heuristics because "silly!" is not a
common filename extension:

```
$ echo "Avoid implicit dependencies" > file.silly!
$ cargo run --quiet --example grep dep file.silly!
>>> external args: ["dep", "file.silly!"]
>>> internal args: ["dep", "file.silly!"]
[ERROR stderr] Error: cannot open file 'file.silly!': File is not available
$
```

Here, the "file.silly!" argument was not replaced by a UUID string, and the
open fails.

Users can always override the heuristics by prefixing paths with `./`:

```
$ cargo run --quiet --example grep dep ./file.silly!
>>> external args: ["dep", "./file.silly!"]
>>> internal args: ["dep", "5f021ccc-88d1-4c08-84c3-c87db42f4815.silly!"]
./file.silly!: Avoid implicit dependencies
```

The path "./file.silly!" was substituted, and the program was able to open it.

Conversely, an argument might be interpreted as a filename when it is
not. Prefixing an argument with `%verbatim:` makes the remainder of the
argument a verbatim argument, even if it looks like a path:

```
$ cargo run --quiet --example grep dep %verbatim:./Cargo.toml
>>> external args: ["dep", "%verbatim:./Cargo.toml"]
>>> internal args: ["dep", "./Cargo.toml"]
[ERROR stderr] Error: cannot open file './Cargo.toml': File is not available
```

Here, "./Cargo.toml" is passed through verbatim, though it cannot be
opened as such because it's not replaced.

`%` at the beginning of an argument is reserved for special features.
 - `%verbatim:` suppresses all inference and passes the rest of the string through as-is.
 - `%read:` translates the path and makes the file only openable for reading.
 - `%write:` translates the path and makes the file only openable for creating, truncating, and writing.
 - `%append:` translates the path and makes the file only openable for appending.

For example, the `cp` example copies from an input file to an output file;
the following gives it explicit access to just read from the input and
just write to the output:

```
$ cargo run --quiet --example cp %read:Cargo.toml %write:foo
>>> external args: ["%read:Cargo.toml", "%write:foo"]
>>> internal args: ["87b21ff3-551a-4a40-b5d4-5357f8ddbc1e.toml", "19fdaf83-bd73-419f-bca3-223606a8f2bf"]
```

Other `%` forms may be added in the future to provide more control for
advanced use cases.

### Why?

This is a much more convenient interface than the `--dir=`. In most cases,
it should let things Just Work without the user having to do WASI-specific
thing. In cases where it doesn't, adding `./` or `%verbatim:` in-band is easier
in many situations than adding out-of-band `--dir=` arguments to the Wasm
runtime.

And it eliminates the temptation to do `--dir=.` or `--dir=/`, which
largely disable the sandbox.

The goal is to make sandboxing convenient enough that people can just
use it, without having it be something they have to opt into, or
configure. VFS's or seccomp sandboxes or user namespaces can be set
up to do similar sandboxing, but they all require up-front
configuration.

Users are already specifying which files they want their programs to
access: it's the files they pass in as arguments. This automatically
allows programs to access just those files, and no others, without
any manual user intervention.

### But what if the application writes a filename into a file?

Not many applications do this, though some do. What will happen?
They'll write a meaningless UUID string, which other programs will
be unable to do anything with.

There are a few potential ways to approach this:

 - UUID strings in stdout are currently translated back into original
   filenames. This functionality could optionally be made available to
   other streams as well.

 - Writing a filename into a file is making an assumption, that readers
   of the file will have the same namespace as the writer. Where we're
   going, this will be an increasingly unreliable assumption, so it's
   worth pushing back on applications and asking whether this is something
   they really need to do.

 - In the future, WASI should have better ways to persist data containing
   references to other data, which should be an alternative to storing
   filenames.
