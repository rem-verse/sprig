# `getbridgeconfig` #

- [x] **Tool Re-Implementation**
- [ ] **Script**

`getbridgeconfig` is a tool that was originally provided as part of the "host
bridge software" suite of tools in the Cafe SDK. It's the underlying tool that
`getbridge` script calls to do the bulk of it's actual work. It's sole job is
to list all the bridges the host has used before, and list it's default bridge.

This tool is currently built to be the same as the tool which being distributed
in at least SDK Version `2.12.13`. If you have another SDK version with an
older copy of getbridgeconfig that acts differently please please reach out so
we can adapt this tool to work with that particular version.

If you're looking for a bug-free version, that follows modern CLI design please
take a look at the `bridgectl` tool. Most of the this tool is just the
subcommands `bridgectl get`.

## Building ##

In order to build you can follow the project instructions, or if you want to
build just this one single package you can use:
`cargo build -p getbridgeconfig` from the root directory of the project to
build a debug version of the application. It will be available at:
`${project-dir}/target/debug/getbridgeconfig`,
or `${project-dir}/target/debug/getbridgeconfig.exe` if you are on windows. If
you want to build a release version that is fully optimized you want to use the
command: `cargo b --release -p getbridgeconfig`. It will be available at:
`${project-dir}/target/release/getbridgeconfig`, or
`${project-dir}/target/release/getbridgeconfig.exe` respectively. This project
should be compatible with any Rust version above: `1.63.0`, although it's
always safest to build with whatever the latest version of Rust is at the time.

## Known Issues ##

There are several known issues with `getbridgeconfig` that have been
intentionally preserved for compatability. We describe the workaround for these
issues that you can use to hopefully get the data you want.

### "Could not retrieve install path" ###

If you run this tool on a particular OS that isn't well known you may run into
an issue where the tool just prints out a message like everytime you try to do
something:

```
ERROR 203: Could not retrieve install path. Is Host Bridge Software installed?
```

This is an error that pops up because we don't know where to store the
`bridge_env.ini` file that contains the list of bridges for your host. Since
the original tool only works on Windows, for other OS's we've just had to guess
on the best path to place this file on other operating systems besides Windows.
However, We're not an expert on the filesystem structure for every OS, nor do
we know every OS in depth; so if we're missing a default path for your OS
please file an issue for us to look into choosing a default path for your OS.

The list of known OS's we have paths for are:

- Windows: `%APPDATA%\bridge_env.ini`
  - You may see this error if the environment variable %APPDATA% isn't set,
    this should always be set by the OS, but if you've unset it manually you'll
    need to set it to the correct value for us to load the file.
- Mac OSX: `~/Library/Application Support/bridge_env.ini`
  - You may see this error if `HOME` isn't set as an environment variable, this
    should again always be set in any shell environment, but if it's unset
    please set it up to be the equivalent of `~`, or your home directory for
    your user.
- Linux/*BSD: `$XDG_CONFIG_HOME/bridge_env.ini` OR `~/.config/bridge_env.ini`
  - You may see this error if you haven't specified an `XDG_CONFIG_HOME`, AND
    you do not have a `$HOME` directory set. If you're not using the X desktop
    system you probably won't have `$XDG_CONFIG_HOME`, so you just want to
    ensure `$HOME` is set. If you're using an X based desktop system, you
    probably want to set `XDG_CONFIG_HOME`.

You can work around this in `bridgectl` with an environment variable, or CLI
argument to manually specify the path to use. For more information see the
`bridgectl` documentation.

### "Segmentation Fault" ###

If you're running the tool trying to gather a list of all the bridges on the
host (e.g. with the `-all` flag), and you DO NOT have a default bridge set, you
will see the tool exit with a message that says `Segmentation fault`. Now this
isn't a _real_ segmentation fault in our tool (while it is in the original),
this is just a custom panic message we use.

This is a known bug in the original binary where my best guess is it assumes
the default bridge is always set if you have any bridges and are printing them.
I'm guessing it just loads it without checking it, and then ends up referencing
invalid memory as a result.
