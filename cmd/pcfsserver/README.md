# `PCFSServer` #

- [x] **Tool Re-Implementation**
- [ ] **Script**

***note: PCFSServer is currently not implemented as we work on finalizing
reversing the server schema.***

PCFSServer is part of the FSEmulation toolchain that specifically allows
querying paths, creating files/folders, etc. With normal unix style paths,
modes, etc.

## Building ##

In order to build you can follow the project instructions, or if you want to
build just this one single package you can use: `cargo build -p pcfsserver`
from the root directory of the project to build a debug version of the
application. It will be available at: `${project-dir}/target/debug/pcfsserver`,
or `${project-dir}/target/debug/pcfsserver.exe` if you are on windows. If you
want to build a release version that is fully optimized you want to use the
command: `cargo b --release -p pcfsserver`. It will be available at:
`${project-dir}/target/release/pcfsserver`, or
`${project-dir}/target/release/pcfsserver.exe` respectively. This project
should be compatible with any Rust version above: `1.63.0`, although it's
always safest to build with whatever the latest version of Rust is at the time.

## Known Issues ##

There are several known issues with `pcfsserver` that have been intentionally
preserved for compatability. We describe the workaround for these issues that
you can use to hopefully get the data you want.

