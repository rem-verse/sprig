# `getbridgetype` #

- [x] **Tool Re-Implementation**
- [x] **Script**

Getbridgetype is a script that was originally provided as part of the "host
bridge software" suite of tools in the Cafe SDK. It was actually never
documented, and was just an internal shell script that would set some
environment variables. From there other scripts would source this one to get
variables setup.

You can find the same function this script provides in the `cat-dev` library
with the `BridgeType` enumeration at the root of the crate.

## Building ##

`getbridgetype` is just implemented as a script so there is no building
required. Simply set the environment variable `SPRIG_RUNNING_FROM_SOURCE=1`,
and then run the scripts directly. Nothing more to it!

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
