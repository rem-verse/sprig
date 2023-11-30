# OSX Packaging #

This directory contains the necessary code to build a package on OSX. In order
to build these you MUST be on an arm64 Mac Machine that is capable of building
the actual project. Assuming you're on a machine that can do that, you're all
good!

## Building ##

In order to build the package for OSX, first build all the rust code on your
machine with: `cargo build --release` from the root of the projects directory.
So binaries are available within `./target/release/`, such as
`./target/release/findbridge`. Assuming the binaries are available at that
location you're good to go!

From there simply run the `package.sh` script from any directory you'd like,
and it'll place a distributed package down in it's own directory!
