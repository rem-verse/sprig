# `dbg-generate-sata-wal-from-pcap` #

- [ ] **Tool Re-Implementation**
- [ ] **Script**

`dbg-generate-sata-wal-from-pcap` is a tool primarly built for debugging PCFS
Sata streams that have been captured in `pcap`, or `pcapng` format. This can
also make it useful for comparing against an official implementation. You can
capture the PCAP of the official SDK, and use it to generate a WAL log that you
can compare with a WAL log from Sprig.

## Building ##

In order to build you can follow the project instructions, or if you want to
build just this one single package you can use: `cargo build -p dbg-generate-sata-wal-from-pcap`
from the root directory of the project to build a debug version of the
application. It will be available at: `${project-dir}/target/debug/dbg-generate-sata-wal-from-pcap`,
or `${project-dir}/target/debug/dbg-generate-sata-wal-from-pcap.exe` if you are on windows. If you
want to build a release version that is fully optimized you want to use the
command: `cargo b --release -p dbg-generate-sata-wal-from-pcap`. It will be available at:
`${project-dir}/target/release/dbg-generate-sata-wal-from-pcap`, or
`${project-dir}/target/release/dbg-generate-sata-wal-from-pcap.exe` respectively. This project
should be compatible with any Rust version above: `1.63.0`, although it's
always safest to build with whatever the latest version of Rust is at the time.
