# `findbridge` #

- [x] **Tool Re-Implementation**
- [ ] **Script**

Findbridge is a tool that was originally provided as part of the "host bridge
software" suite of tools in the Cafe SDK. It's sole job was to scan the network
to try, and identify all of the development kits (specifically the "cat-dev"'s)
that were running. From there a user could load up the consoles web page, or
use other tools to interact with the console.

It is one of the few tools that is not dependent on a cafe environment in order
to run, and is in fact one of the few tools that usually always lives in
`PATH`.

This tool is currently built to be the same as version `5.1` of the
`findbridge` tool which being distributed in at least SDK Version `2.12.13`.
If you have another SDK version with an older copy of findbridge that acts
differently please please reach out so we can adapt this tool to work with
that particular version.

If you're looking for a bug-free version, that follows modern CLI design please
take a look at the `bridgectl` tool. Most of the this tool is just the
subcommands `bridgectl ls`, and `bridgectl get`.

## Building ##

In order to build you can follow the project instructions, or if you want to
build just this one single package you can use: `cargo build -p findbridge`
from the root directory of the project to build a debug version of the
application. It will be available at: `${project-dir}/target/debug/findbridge`,
or `${project-dir}/target/debug/findbridge.exe` if you are on windows. If you
want to build a release version that is fully optimized you want to use the
command: `cargo b --release -p findbridge`. It will be available at:
`${project-dir}/target/release/findbridge`, or
`${project-dir}/target/release/findbridge.exe` respectively. This project
should be compatible with any Rust version above: `1.63.0`, although it's
always safest to build with whatever the latest version of Rust is at the time.

## Known Issues ##

There are several known issues with `findbridge` that have been intentionally
preserved for compatability. We describe the workaround for these issues that
you can use to hopefully get the data you want.

### CAT-DEV on Separate VLANs/Subnets/Networks Not Showing Up ###

The `findbridge` tool utilizes UDP broadcast packets in order to "find" all
the devices on the network. This is pretty reasonable, but it does mean you can
only find devices that are on the same local network, same subnet, and same
VLAN as the host computer. There is no way for our tool to fundamenetally fix
this as it's just how the UDP Broadcast address works. However, you may be like
some of the developers of `sprig` where you want your CAT-DEV on a network
VLAN/Subnet that doesn't have internet access, while your main PC is on a
segment that has full network access. What do you do here?

The best, and really only way to solve this is to "forward" the broadcasts
between networks. This requires a PC/Raspberry Pi/etc. that is connected to
both networks at the same time, and can run a program to forward both ways. You
could even in theory forward across something like [wireguard](https://www.wireguard.com/)
to forward it over the internet!

Some kernels can automatically forward packets, other times you might be able
to use a tool like: <https://github.com/udp-redux/udp-broadcast-relay-redux>
to relay between the two. If you were using udp-broadcast-relay-redux you'd
ensure your pc connected to both networks is running the following two
commands (note: you may need to add more/change the port being used if you are
not using a standard setup, some things may try to use the ATAPI configured
port, which is also by default 7974, but can be changed):

```sh
./udp-broadcast-relay-redux --id 1 --port 7974 --dev <network-one-interface> --dev <network-two-interface>
```

This way packets are being forwarded in both directions both the broadcasts
out, and the broadcasts inbound. ***BOTH*** are needed in order for full
compatability. The port will *ALWAYS* be the same. You can also add as many
devices as you need incase you need to broadcast across multiple packets.

### MAC Address Only Searchable With `-mac` Flag Conflicting With Help Text Documentation ###

Although the help text for `findbridge` shows the following line:

```
findbridge [options] <name> or <ip_address> or <mac_addr>
```

If you try specifying the mac address without specifying the `-mac` flag right
before the value like: `findbridge -detail -list -mac <mac address>`, the tool
will instead do a lookup on the name of the bridge. IT WILL NOT SEARCH BY MAC.
This isn't really a bug per-say, just some poorly written help text, but we
wanted to mention it here as we certainly made the mistake of thinking it would
work.

### Detailed Information Not Showing Up When Searching By Name ###

`findbridge` not only has the ability to list all bridges on the network, but
can "find" a very specific bridge. When you search for a specific bridge by
name, AND ONLY BY NAME the "detailed" information is never fetched, even if you
specify the `-detail` flag. Instead in list view it will show "Unknown" values,
and in columnar view it will just display nothing:

```text
$ findbridge 00-25-5C-BA-5A-00 -detail -list

Bridge name        : '00-25-5C-BA-5A-00'
IP address         : 192.168.7.40
MAC address        : 00:25:5C:BA:5A:00
FPGA image version : 1352071
Firmware version   : 0.0.14.80
SDK version        : Unknown
Boot Mode          : Unknown
Power Status       : Unknown
```

```text
$ findbridge 00-25-5C-BA-5A-00 -detail

00-25-5C-BA-5A-00               192.168.7.40    0.0.14.80  13052071  00:25:5C:BA:5A:00
```

We're gonna be honest, we have no idea why it just doesn't fetch this
information. Clearly it still tries to display it, so why doesn't it also use
that flag for fetching detailed info? Beats us! After all it can still fetch
detailed information if you search by mac address, or by ip. The quickest way
to find this information using `findbridge` as opposed to `bridgectl` would be
to first do a lookup (non-detailed) based on it's name: `findbridge <name>`,
and then pass the IP, to do a detailed lookup: `findbridge <ip> -detail`:

```
$ findbridge 00-25-5C-BA-5A-00
00-25-5C-BA-5A-00              : 192.168.7.40

$ findbridge 192.168.7.40 -detail

00-25-5C-BA-5A-00               192.168.7.40    0.0.14.80  13052071  00:25:5C:BA:5A:00  2.12.13 PCFS  OFF
$ findbridge 192.168.7.40 -detail -list

Bridge name        : '00-25-5C-BA-5A-00'
IP address         : 192.168.7.40
MAC address        : 00:25:5C:BA:5A:00
FPGA image version : 1352071
Firmware version   : 0.0.14.80
SDK version        : 2.12.13
Boot Mode          : PCFS
Power Status       : OFF
```

We recommend using the IP over the MAC Address, as an IP will just send a
packet directly to the device, and using a MAC Address will cause a full scan
of the network (with packets to all BROADCAST addresses), and is techincally
slower.

### `findbridge` Exiting With Error Code On Success ###

This is unfortunately "expected" behavior, and also in that case of "not really
a bug, but just is incredibly unexpected". The original tool will ONLY exit
successfully when finding one specific bridge (e.g. not using the `-all` flag),
and when it successfully finds that bridge. In every other help case, including
doing something as simple as displaying the help text it will exit with `-1`.
