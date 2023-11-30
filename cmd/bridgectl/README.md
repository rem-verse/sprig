# `bridgectl` #

- [ ] **Tool Re-Implementation**
- [ ] **Script**

`bridgectl` is a new tool that we built that aims to paper over a lot of the
weirdness with the existing Host Bridge Software tools like `findbridge`,
`getbridge`, `setbridge`, etc. while still providing the same exact functions,
and more. While also following more modern CLI design principles.

## Building ##

In order to build you can follow the project instructions, or if you want to
build just this one single package you can use: `cargo build -p bridgectl`
from the root directory of the project to build a debug version of the
application. It will be available at: `${project-dir}/target/debug/bridgectl`,
or `${project-dir}/target/debug/bridgectl.exe` if you are on windows. If you
want to build a release version that is fully optimized you want to use the
command: `cargo b --release -p bridgectl`. It will be available at:
`${project-dir}/target/release/bridgectl`, or
`${project-dir}/target/release/bridgectl.exe` respectively. This project
should be compatible with any Rust version above: `1.63.0`, although it's
always safest to build with whatever the latest version of Rust is at the time.
