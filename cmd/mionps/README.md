# `mionps` #

- [x] **Tool Re-Implementation**
- [ ] **Script**

`mionps` is a tool that was originally provided as part of the suite of tools
in the Cafe SDK. It is a tool that gets used in several scripts to read from
the "parameter space" of a MION board. While most of the parameters in the
parameter space are not known, this is how things like the SDK version are
fetched.

This tool is currently built to be the same as the tool which being distributed
in at least SDK Version `2.12.13`. If you have another SDK version with an
older copy of `mionps` that acts differently please please reach out so we
can adapt this tool to work with that particular version.

If you're looking for a bug-free version, that follows modern CLI design please
take a look at the `bridgectl` tool. You can take a look at `get-params`,
`set-params`, `dump-params`, etc.

## Building ##

In order to build you can follow the project instructions, or if you want to
build just this one single package you can use:
`cargo build -p mionps` from the root directory of the project to
build a debug version of the application. It will be available at:
`${project-dir}/target/debug/mionps`,
or `${project-dir}/target/debug/mionps.exe` if you are on windows. If
you want to build a release version that is fully optimized you want to use the
command: `cargo b --release -p mionps`. It will be available at:
`${project-dir}/target/release/mionps`, or
`${project-dir}/target/release/mionps.exe` respectively. This project
should be compatible with any Rust version above: `1.63.0`, although it's
always safest to build with whatever the latest version of Rust is at the time.

## Known Issues ##

There are no current known "issues" with `mionps`, although it's CLI argument
parser is very finicky. We've listed it down below just incase, but it isn't
neccissarily an issue.

### "missing offset parameter" with verbose flag ###

If you try specifying a verbose flag (e.g. `-v`), that comes after specifying
a particular ip, you will get an error message about "missing offset
parameter". This is because the original CLI parser, assumes anything after the
first argument is another argument. So it takes the `-v` as an argument, not as
the verbose flag. This then exits with an error.
