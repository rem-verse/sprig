# `setbridgeconfig` #

- [x] **Tool Re-Implementation**
- [ ] **Script**

`setbridgeconfig` is a tool that was originally provided as part of the "host
bridge software" suite of tools in the Cafe SDK. It's the underlying tool that
`setbridge` script calls to do the bulk of it's actual work. It's job is adding
new bridges to the host environment, and setting the default bridge to be used.

This tool is currently built to be the same as the tool which being distributed
in at least SDK Version `2.12.13`. If you have another SDK version with an
older copy of setbridgeconfig that acts differently please please reach out so we
can adapt this tool to work with that particular version.

If you're looking for a bug-free version, that follows modern CLI design please
take a look at the `bridgectl` tool. Most of the this tool is just the
subcommands `bridgectl add`, and `bridgectl set-default`.

## Building ##

In order to build you can follow the project instructions, or if you want to
build just this one single package you can use:
`cargo build -p setbridgeconfig` from the root directory of the project to
build a debug version of the application. It will be available at:
`${project-dir}/target/debug/setbridgeconfig`,
or `${project-dir}/target/debug/setbridgeconfig.exe` if you are on windows. If
you want to build a release version that is fully optimized you want to use the
command: `cargo b --release -p setbridgeconfig`. It will be available at:
`${project-dir}/target/release/setbridgeconfig`, or
`${project-dir}/target/release/setbridgeconfig.exe` respectively. This project
should be compatible with any Rust version above: `1.63.0`, although it's
always safest to build with whatever the latest version of Rust is at the time.

## Known Issues ##

There is one known issue with `setbridgeconfig` that has been
intentionally preserved for compatability. We describe the workaround for this
issue that you can use to hopefully get the data you want.

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
However, we're not an expert on the filesystem structure for every OS, nor do
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
