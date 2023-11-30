# `setbridge` #

- [x] **Tool Re-Implementation**
- [x] **Script**

Setbridge is a script that was originally provided as part of the "host bridge
software" suite of tools in the Cafe SDK. It's job is to add new bridges to the
system (adding them to the `bridge_env.ini` file, and setting the environment
variables to use them in the current shell), and allow configuring the
"default" bridge. It is also how you 'delete' bridges from the `bridge_env.ini`
file.

If you're looking for a tool that does all the same things, but is faster,
follows modern CLI design, and more take a look at `bridgectl`. Most of this
tool has been replicated by `bridgectl add`, and `bridgectl set-default`.

## `setbridge` vs `setbridge.bat` ##

The original Cafe SDK's that I have, have two versions of `setbridge`, and
`setbridge.bat`. This makes sense considering the Cafe SDK started introducing
"CafeX" without dependencies on cygwin and a bash shell. They wanted to write
software that just ran on Windows. HOWEVER, unlike `getbridge`, or other tools
that have these two implementation scripts (one if you're using cafe, one if
you're using "CafeX") that do the same thing, `setbridge` is interesting
because at least as of SDK version 2.12.13, `setbridge`, and `setbridge.bat` DO
NOT do the same thing. At least as far as I can tell from a black box reversing
perspective.

`setbridge.bat` acts like a user can call it directly as opposed to `setbridge`
which does not let a user call it directly. `setbridge.bat` also does not run
a series of `sessionmanagerutil.exe` when the session manager env-var is set
to 1. Where as `setbridge` does spin up a series of `sessionmanagerutil.exe`
commands.

This puts us in kind of a tough spot, when a user could potentially be running
either script. Plus could be sourcing it, or running it as a command. We try
to take the MOST compatible approach where BOTH scripts can be sourced, or run
as a command. Not to mention BOTH scripts follow the `sh` behavior, and run the
`SessionManagerUtil.exe` commands by default. If you REALLY want to emulate the
cafex `setbridge` batch script you can set the environment variable:
`SPRIG_USE_CAFEX_SETBRIDGE=1`. This will use the "cafex" version where we do not
run the Session Manager commands.

## Building ##

`setbridge` is just implemented as a script so there is no building required.
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
