# Windows Packaging Building a MSI #

This directory contains the necessary code (and windows specific licenses) in
order to build an MSI for distribution. Even if we don't fully distribute these
MSIs anywhere really yet. These MSI's also aren't going through any sort of
signing infrastructure even though we absolutely do want to sign at some point.

## Building a Package ##

### Setting up Dependencies ##

In order to start building our packages you're going to need to install Wixv4,
which you can find instructions on how to do so:
<https://wixtoolset.org/docs/intro/>. From there cd into the directory with
this README, and run the following two commands from your terminal:

```shell
wix extension add WiXToolset.UI.wixext
wix extension add WiXToolset.Util.wixext
```

This should add a `.wix/extensions/` directory right next to this README.md. At
this point you have successfully setup package specific dependencies. And if
you're on a Windows host you can build the rust binaries you're all set!

### Building a Package ###

First make sure to build the entire codebase by running `cargo build --release`
on a Windows x64 host machine, making sure the binaries are available in the
`./target/release/` directory (e.g. `./target/release/findbridge.exe`).
To build a package, from within the directory that this README is in you can run:

```shell
wix build sprig.wxs -ext WiXToolset.Util.wixext -ext WiXToolset.UI.wixext -defaultcompressionlevel high -arch "x64" -bindpath "../../" -arch "x64" -bindpath "../../"
```

This will create an MSI file within this directory that you can use!