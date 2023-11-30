# `getbridge` #

- [x] **Tool Re-Implementation**
- [x] **Script**

Getbridge is a script that was originally provided as part of the "host bridge
software" suite of tools in the Cafe SDK. It's job is to display the current
bridge that is actively being used, though it also has a tool to display every
single CAT-DEV that has ever been used by the host system. In reality it's just
a thin wrapper around a tool called `getbridgeconfig.exe` that isn't
documented.

It's list of all running hosts is purely a file based lookup on the host pc,
and can show incorrect/outdated information. If you want to fetch updated
information the idea is you'd use `findbridge` to list new details.

If you're looking for a tool that does all the same things, but is faster,
follows modern CLI design, and more take a look at `bridgectl`. Most of this
tool has been recreated within `bridgectl get`.

## Building ##

`getbridge` is just implemented as a script so there is no building required.
Simply set the environment variable `SPRIG_RUNNING_FROM_SOURCE=1`, and then
run the scripts directly. Nothing more to it!

If you're on Windows you want to run the scripts under the `pwsh` directory, 
otherwise run the scripts from the `sh` directory.

### Packaging Notes ###

The scripts when not running with `SPRIG_RUNNING_FROM_SOURCE` expect to not
only be in `PATH`, but also that they are all placed in the same directory
together. This is to try and help with any path issues where you may have
multiple versions of sprig all installed at the same time, even though that
should ideally never happen we want to try and be as compliant as possible.

This script ***does*** depend on core-utils, but should be compatible with both
GNU & BSD coreutils. It also should be capable of running in just a normal `sh`
environment (e.g. no dependency on bash, or any of it's versions).
