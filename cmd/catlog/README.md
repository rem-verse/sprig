# `CatLog` #

***note: CatLog is currently not implemented as we work on identifying a
cross platform accessible UI framework.***

- [x] **Tool Re-Implementation**
- [ ] **Script**

`catlog.exe` was a small simple application distributed only in source form.
It simply allowed viewing logs on the serial port, and allows saving them to
a file.

This tool is currently built to be the same as the tool which being distributed
in at least SDK Version `2.12.13`. If you have another SDK version with an
older copy of `catlog` that acts differently please please reach out so we
can adapt this tool to work with that particular version.

As a side note, if you're looking for an equivalent to this tool but in CLI
form feel free to check out `bridgectl`.

## Building ##

In order to build you can follow the project instructions, or if you want to
build just this one single package you can use:
`cargo build -p catlog` from the root directory of the project to
build a debug version of the application. It will be available at:
`${project-dir}/target/debug/catlog`,
or `${project-dir}/target/debug/catlog.exe` if you are on windows. If
you want to build a release version that is fully optimized you want to use the
command: `cargo b --release -p catlog`. It will be available at:
`${project-dir}/target/release/catlog`, or
`${project-dir}/target/release/catlog.exe` respectively. This project
should be compatible with any Rust version above: `1.63.0`, although it's
always safest to build with whatever the latest version of Rust is at the time.

## Known Issues ##

There are no known issues with `catlog`.
