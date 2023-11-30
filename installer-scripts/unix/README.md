# Unix Packaging #

This directory contains the packaging configuration for building amd64 pacakges
for a series of unix systems, specifically:

- `apk` for Alpine
- `deb` for Debian based systems (Debian, Ubuntu, etc.)
- `rpm` for systems like Fedora, CentOS, etc.
- `.pkg.tar.zst` for ArchLinux and derivatives.

These are all built with the [NFPM][nfpm] tool only. Ideally in the future we
start signing these packages, and actually distributing them somewhere.
However, we also probably wanna be a bit further along before we start signing
these packages & building them.

## Building Packages ##

Assuming you're on an amd64 unix based system (of any kind), first run:
`cargo build --release` in the root of this project to fully build all projects
in release mode (with all the optimizations applied). Ensuring the binaries are
available in `<project root>/target/release/` folder. For example:
`<project root>/target/release/bridgectl` should exist after
`cargo build --release`. These should be linux binaries for the amd64
architecture.

From there please make sure you've installed [NFPM][nfpm], and run:
`nfpm package -p <package type>` from within the directory this README is
located in. Where package type can be one of:

- `deb` for a debian package.
- `apk` for an alpine linux package.
- `rpm` for an rpm package.
- `archlinux` for an arch linux based package.

[nfpm]: https://nfpm.goreleaser.com/