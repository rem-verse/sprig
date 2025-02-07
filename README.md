# Sprig #

***note: this current project is far far far from done, and is one of many spare
time projects. It is mostly only useful for those interacting with cat-dev's at
this point.***

A re-implementation of Wii-U Development tools, without the fuss. This attempts
to recreate the real SDK that can run on any OS (without needing to setup
cygwin, or needing a windows machine), as well as 'better' versions of the tool
that comply to more modern CLI/App Standards. As part of sprig I do also run
cygwin mirrors, and provide support for setting up the official sdk. But
hopefully this project can fully eclipse the need for the official Cafe SDK.

***As a side note: if you're interested in testing something on a cat-dev for
something related to preservation, and don't have one. PLEASE reach out, I'd
be more than happy to use my hardware to test things for you.***

## Why The Name Sprig? ##

The name "sprig" was chosen after "sprigatito" the "Weed Cat Pokemon", because
not only was the code name for the Wii-U "CAT" (cat-dev/cat-r/etc.), and the
fact "Cat Bridge - Dev" is shortened to CBD (thus 'weed'). Plus Sprig is just a
fun word to say.

## What Parts Are Re-Implemented ##

As mentioned at the top of this repository ALMOST ALL of the tools here are
NOT re-implemented _yet_. I'm working on it bit by bit, but to be clear it is
not the only thing I'm working on, or always my top project. It's very much as an
on needed basis til I can finish other things (and in the meantime I'd love help
from anyone willing to contribute).

The end goal is to offer a *complete* port of every single tool that was
officially part of the Cafe SDK developed by Nintendo, plugins to use other
compilers besides MULTI (so we don't need the license keys!), and a suite of
tools designed from the ground up to offer a pleasent development experience.
As some of the CLI choices of the Cafe SDK are not great choices looking back.

### Overarching Cafe Tools ###

Official Tool Replacements:

- [ ] `cafe.bat`
- [ ] `cafex_env.bat`
- [ ] `cafex`

Sprig Custom Tooling:

- [ ] `mochiato`: our replacement for `cafe.bat`/`cafex_env.bat`

### Host Bridge Tools ###

The "Host Bridge" tools are a series of tools used for setting up a connection
to a CAT-DEV Unit on your local network. These tools are all documented in the
Cafe-SDK documentation underneath: "Cafe SDK Basics > Development Cycle >
Run Applications > HostBridge Tools".

Official Tool Replacements:

- [x] `findbridge`: a tool to list all the bridges on your local network who
                    your PC can see.
- [x] `getbridge`: print either the current bridge, or all known bridges.
  - [x] `getbridgetype`: an internal script that sets environment variables
                          that `getbridge` uses.
  - [x] `getbridgeconfig`: the actual executable that does the BULK of work for
                            `getbridge` when not just echoing environment
                            variables.
- [-] `setbridge`: set the bridge to use for your active session, or set the
                    default so you don't have to set it everytime.
  - [x] `setbridgeconfig`: the actual executable that `setbridge` ends up
                            reaching out too.
  - [ ] `SessionManagerUtil`: _seems like a utility to manage multiple cat-dev
                               PCFS connections at once._
- [ ] `hostdisplayversion`: Display the current emulated Host Bridge
                            installation version, and the firmware installed
                            on your actual CAT-DEV. It is typically only used
                            for diagnostics.
  - [ ] `FSEmul`: FSEmul is the 'core' proccess for handling emulation of
                  various filesystem components for the CAT-DEV. Specifically
                  FSEmul handles various block level protocols (SDIO/ATAPI),
                  and provides information to `PCFSServer`
- [ ] `updatebridges`: a command used to update the firmware on a particular
                       Host Bridge.
- [ ] `imageuploader`: allow uploading mastered `WUMAD`/`WUM`'s to the internal
                        HDD of a CAT-DEV.

Sprig Custom Tooling:

- [-] `bridgectl`: our replacement tool that wraps all the bridge commands, and
                    host-bridge utilities into a single tool.

### Cat-Dev Bridge Internal Tools ###

There are some tools that are not explicitly mentioned being tools in any parts
of the developer facing documentation, however they are used for interacting
with that cat-dev bridges a developer would have used. Sometimes, these tools
were also used to build things ontop of them (e.g. `mionps` is used during
`cafe.bat`).

- [ ] `CatLog`: a port of the the csharp "CatLog" utility who's source was
      included in some of the cafe sdk releases, which receives logs over
      the serial port.
- [x] `mionps`: a tool used to fetch the "parameter space"
      from a particular MION board. It is not clear what the parameter space
      is, but it contains a whole bunch of configuration values you can
      get/set.
- [x] `mionparameterspace`: a tool like `mionps`, but it has different output
      and is probably an older version of `mionps` that couldn't be removed
      cause someone depended on it's behavior somewhere.

## Building ##

Building these tools for yourself (and not installing from some package when we
start distributing pre-built artifacts) you will require whatever the latest
version of stable Rust is at the time the source code was published.

You can follow instructions from <https://rust-lang.org>, and
<https://rustup.rs>. To install a working rust toolchain locally on your
machine. I personally recommend using [rustup](https://rustup.rs) as it'll be
the easiest to update in the future.

Then run `cargo build` from the root directory of this project to build debug
versions of these tools. The built binaries will be placed in
`${PROJECT_DIR}/target/debug/${tool-name}`. ***NOTE: not all tools are binaries
that need to be built. Some are just simple scripts.*** For these 'simple
scripts' they are just locally located in the directory their located, and you
can just run them directly. On windows you can run the scripts located in the
`pwsh` directories, on anything else you can use the scripts located in the
`sh` directories. *NOTE: there may be extra steps consult each projects README
for more information.*

### Building the Installable Packages ###

*note: the installable packages are not being fully tested yet, so expect breakages.*

If you actually want to build packages you'll need to not only be the OS that
you _want_ to package for. You'll also potentially need extra tools depending
on the OS you're using:

- Windows: Please install [wix 4](https://wixtoolset.org/).
- Mac OS X: Please ensure you have `pkgbuild`, and `productbuild` installed you
            may need to install [XCode](https://apps.apple.com/us/app/xcode/id497799835?mt=12).
- *Nix: Please install [NFPM](https://nfpm.goreleaser.com/).

From there you can run: `./installer-scripts/<os-type>/<install scripts>`, and
read: `./installer-scripts/<os-type>/README.md` to get instructions on how to
build things for your OS.
