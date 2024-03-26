# `mionparamspace` #

- [x] **Tool Re-Implementation**
- [ ] **Script**

`mionparamspace` is a tool that was originally provided as part of the suite of
tools in the Cafe SDK. It seems to be an older, less functional version of the
`mionps` tool (e.g. it doesn't let specifying a timeout). While most scripts
seem to just use `mionps` rather than this command we have ported it over so
anything using this older version of the tool keeps working.

This tool is currently built to be the same as the tool which being distributed
in at least SDK Version `2.12.13`. If you have another SDK version with an
older copy of `mionparamspace` that acts differently please please reach out so we
can adapt this tool to work with that particular version.

If you're looking for a bug-free version, that follows modern CLI design please
take a look at the `bridgectl` tool. You can take a look at `get-params`,
`set-params`, `dump-params`, etc.

## Building ##

In order to build you can follow the project instructions, or if you want to
build just this one single package you can use:
`cargo build -p mionparamspace` from the root directory of the project to
build a debug version of the application. It will be available at:
`${project-dir}/target/debug/mionparamspace`,
or `${project-dir}/target/debug/mionparamspace.exe` if you are on windows. If
you want to build a release version that is fully optimized you want to use the
command: `cargo b --release -p mionparamspace`. It will be available at:
`${project-dir}/target/release/mionparamspace`, or
`${project-dir}/target/release/mionparamspace.exe` respectively. This project
should be compatible with any Rust version above: `1.63.0`, although it's
always safest to build with whatever the latest version of Rust is at the time.

## Known Issues ##

There are no functional issues with `mionparamspace`, but there is one known
issue with output being displayed in the tool.

### Verbose Flag prints wrong ip ###

When running the tool with the verbose flag (`-v`), you will see an output line
that starts with: `"mionparamspace: Success setting value"`, you will notice
the final byte of the IP isn't the real final byte of the IP you're connected
too. Instead you'll notice it's the offset you specified to set. This is an
issue in the original tool, but you can rest assured it did set on the IP you
specified.
