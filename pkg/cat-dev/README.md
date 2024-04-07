<div align="center">

# `cat-dev` #

A library used for interacting with [CAT-DEV](https://wiki.raregamingdump.ca/index.php?title=CAT-DEV)
also sometimes referred to as the "cat-dev bridge". A CAT-DEV is actually two
pieces under the hood, a "Host Bridge" which is what we actually interact with,
and the actual "Cafe Main Board" (if you have a legal copy of the Cafe SDK
v2.12.13 -- you can look at the doc page at
`/system/docs/man/en_us/dev/catdev/prerequisite/DevelopmentEnvironment.html`
to find an image explaining this flow).

  <picture>
    <image alt="A photo of a CAT-DEV v3 Console sitting in a makeshift container." width=640 height=480 src="https://c.reml.ink/images/github/sprig/cat-dev.jpg"></image>
  </picture>

</div>

## Stability ##

This crate is currently pre-1.0. We will do our best to minimize breaking
changes to the library, but there is still many parts of the cat-dev we have
not fully figured out. As such we're very hesitant to make any promises about
the stability of these APIs, or that we'll follow SEM-VER until we've at the
very least figured all of that out. If you are ever affected by this, or
concerned about this please reach out on our github repository. We do want to
try, and make it as smooth as possible.

## "Probably Don't Want Functions" ##

***Please double check the documentation before using functions***

Because this library is developed in close relation to re-implementations of
very old, very buggy CLI tools there are some functions that exist for the sole
purpose of recreating these buggy, or poorly displaying CLIs. These functions
will be marked in their documentation.

One key thing to watch out for: `_with_logging_hooks` these functions don't
magically enable logging, they simply allow hooks for running `println!`, and
`print!`'s completely outside of the logging infrastructure. For tool
reimplementations that need to match their output EXACTLY and thus can't use
the normal logging infrastructure.

## Usage ##

Here are some basic use cases, this not all encompassing but may be helpful for
you.

### `findbridge`-esque style discovery ###

You can discover all the bridges on your host in the following fashion:

```rust,no_run
use cat_dev::mion::discovery::discover_bridges;

/// Fetching detailed fields grabs a few extra bits of information, these are
/// all prefixed with `detailed_` in the returned info structures, but for
/// completeness sake the fields are:
///
/// - `detailed_sdk_version`
/// - `detailed_boot_type`
/// - `detailed_is_cafe_on`
///
/// These all return options that will only be populated if
/// `fetch_detailed_fields` is marked as true.
async fn stream_bridges(fetch_detailed_fields: bool) {
  let mut channel_to_stream_bridges = discover_bridges(
    fetch_detailed_fields,
    None,
  ).await.expect("Failed to discover bridges!");

  // This will block for a potentially "long-ish" (like 10 seconds) time.
  //
  // Our CLI tool avoids this by timing out if we don't receive a bridge in enough time.
  // You can use `discover_and_collect_bridges` with an "early timeout" to get a much
  // faster loop here. Though you do have to wait for all of the results to come in.
  //
  // FWIW our timeout in the CLI Re-implementations is 3 seconds.
  while let Some(bridge) = channel_to_stream_bridges.recv().await {
    println!("Found bridge: {bridge}");
  }
}
```

### `getbridge`-esque style finding a specific bridge ###

You can get information about a specific bridge like so:

```rust,no_run
use cat_dev::mion::discovery::{find_mion, MIONFindBy};
use std::time::Duration;

async fn find_a_mion_by_name(
  name: String,
  fetch_detailed_fields: bool,
  early_search_timeout: Option<Duration>,
) {
  if let Some(bridge) = find_mion(
    // You can also search by IP / Mac Address.
    MIONFindBy::Name(name),
    fetch_detailed_fields,
    // By default we wait the full 10 seconds to try and find a bridge with
    // this name, in many cases though if you don't get a response in under
    // 3, or sometimes even 1 second you're not gonna find it. If you don't
    // wanna wait the full 10 seconds you can specify an optional early
    // timeout.
    early_search_timeout,
    None,
  ).await.expect("could not conduct a search for a mion.") {
    println!("Found bridge: {bridge}");
  } else {
    panic!("No bridge present with that name :(");
  }
}
```

### Getting the Systems Default Bridge information ###

Chances are if your interacting with a system you probably have a "default
bridge", which you want to use by default if no name was provided. You can
get this bridges information from the host state.

```rust,no_run
use cat_dev::BridgeHostState;

async fn get_default_mion() {
  let host_state = BridgeHostState::load()
    .await
    .expect("Could not load the state file of the bridges the computer knows about.");

  if let Some((bridge_name, optional_bridge_ip)) = host_state.get_default_bridge() {
    if let Some(bridge_ip) = optional_bridge_ip {
      println!("Found default bridge {bridge_name} @ {bridge_ip}!");
    } else {
      println!("Found default bridge named: {bridge_name}!");
      println!("No ip was saved though perhaps try searching for it?");
    }
  } else {
    println!("There was no default bridge :(");
  }
}
```
