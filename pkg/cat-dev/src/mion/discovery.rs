//! APIs for discovering Cat-Dev Bridge's, and more specifically their MION
//! boards.
//!
//! There are two main groups of methods for attempting to find MIONs:
//!
//! 1. [`discover_bridges`], [`discover_bridges_with_logging_hooks`],
//!    [`discover_and_collect_bridges`], and
//!    [`discover_and_collect_bridges_with_logging_hooks`] incase you
//!    want to output values as you discover mions (processing them in a
//!    stream), or if you want to collect all the values in a single vector
//!    at the very end.
//! 2. [`find_mion`] which finds a specific MION board based on one of the
//!    identifiers we know how to search for. *NOTE: in some cases this can
//!    lead to a full scan. See the API information for details.*
//!
//! It should be noted you can only find bridges that are on the same broadcast
//! domain within your local network. In general this means under the same
//! Subnet, and VLAN (unless your repeating broadcast packets across VLANs).
//!
//! If you are in different VLANs/Subnets and you do have the ability to run
//! a repeater heading in BOTH directions (both are required for all bits of
//! functionality!), you want to broadcast the port
//! [`crate::mion::MION_CONTROL_PORT`] aka 7974. Otherwise things will not
//! work.

use crate::{
	errors::{CatBridgeError, NetworkError},
	mion::proto::{
		control::{MionIdentity, MionIdentityAnnouncement},
		MION_ANNOUNCE_TIMEOUT_SECONDS, MION_CONTROL_PORT,
	},
};
use bytes::{Bytes, BytesMut};
use fnv::FnvHashSet;
use futures::stream::{unfold, StreamExt};
use mac_address::MacAddress;
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use std::{
	fmt::{Display, Formatter, Result as FmtResult},
	hash::BuildHasherDefault,
	net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
};
use tokio::{
	net::UdpSocket,
	sync::mpsc::{unbounded_channel, UnboundedReceiver},
	task::JoinSet,
	time::{sleep, Duration, Instant},
};
use tracing::{debug, error, warn};

/// A small wrapper around [`discover_bridges`] that collects all the results
/// into a list for you to parse through.
///
/// This will in general be slower than the `findbridge` cli tool, or even
/// `bridgectl` because it will attempt to wait for the maximum amount of time
/// in order to let any slow MIONs respond. Where as `bridgectl` (by default),
/// and `findbridge` will attempt to exit early if they're not seeing a lot of
/// responses on the network. To replicate their speed, and behaviour you can
/// pass an early timeout of 3 seconds.
///
/// ## Errors
///
/// See the error notes for [`discover_bridges`].
pub async fn discover_and_collect_bridges(
	fetch_detailed_info: bool,
	early_timeout: Option<Duration>,
) -> Result<Vec<MionIdentity>, CatBridgeError> {
	discover_and_collect_bridges_with_logging_hooks(
		fetch_detailed_info,
		early_timeout,
		noop_logger_interface,
	)
	.await
}

/// A small wrapper around [`discover_bridges`] that collects all the results
/// into a list for you to parse through with extra logging handlers.
///
/// You ***probably*** don't want to call this directly, and instead call
/// [`discover_bridges`] this can mainly be used for folks who need to create
/// CLI tools with hyper-specific `println!`'s that don't use the normal
/// [`tracing`] crate, or need some custom hooks to process data.
///
/// This will in general be slower than the `findbridge` cli tool, or even
/// `bridgectl` because it will attempt to wait for the maximum amount of time
/// in order to let any slow MIONs respond. Where as `bridgectl` (by default),
/// and `findbridge` will attempt to exit early if they're not seeing a lot of
/// responses on the network. To replicate their speed, and behaviour you can
/// pass an early timeout of 3 seconds.
///
/// ## Errors
///
/// See the error notes for [`discover_bridges`].
pub async fn discover_and_collect_bridges_with_logging_hooks<InterfaceLoggingHook>(
	fetch_detailed_info: bool,
	early_timeout: Option<Duration>,
	interface_logging_hook: InterfaceLoggingHook,
) -> Result<Vec<MionIdentity>, CatBridgeError>
where
	InterfaceLoggingHook: Fn(&'_ Addr) + Clone + Send + 'static,
{
	let mut recv_channel =
		discover_bridges_with_logging_hooks(fetch_detailed_info, interface_logging_hook).await?;

	let mut results = Vec::new();
	loop {
		tokio::select! {
			opt = recv_channel.recv() => {
				let Some(identity) = opt else {
					// No more identities being received.
					break;
				};
				// The same identity could be broadcast multiple times!
				if !results.contains(&identity) {
					results.push(identity);
				}
			}
			() = sleep(early_timeout.unwrap_or(Duration::from_secs(MION_ANNOUNCE_TIMEOUT_SECONDS * 2))) => {
				break;
			}
		}
	}
	Ok(results)
}

/// Discover all the Cat-Dev Bridges actively on the network.
///
/// NOTE: This will only find MIONs within the time window of
///   [`crate::mion::MION_ANNOUNCE_TIMEOUT_SECONDS`].
///   To stop scanning for broadcasts early, simply close the receiving end of
///   the channel.
///
/// This is what most users will actually want to mess with, as it simply logs
/// using tracing, and returns the stream, AS devices are discovered. You might
/// also want to use the api: [`discover_and_collect_bridges`]. Which handles
/// all the scanning for you for 10 seconds, and then gives you the full list
/// of discovered MIONs. Which also accepts an optional early timeout so you
/// don't gotta wait the full seconds if you know you're not having some
/// respond slowly.
///
/// *note: if you have multiple interfaces on the same network it is possible
/// with this function to receive the same interface multiple times. you should
/// handle any de-duping on your side!*
///
/// There are also two sister functions [`discover_bridges_with_logging_hooks`]
/// and [`discover_and_collect_bridges_with_logging_hooks`]. Which are used by
/// the command line tool `findbridge` in order to match the output of the
/// original tools EXACTLY. For most users you probably don't want those
/// logging hooks, as they ALREADY get piped through [`tracing`].
///
/// ## Errors
///
/// - If we fail to spawn a task to concurrently look up the MIONs.
/// - If any background-task fails to create a socket, and broadcast on that
///   socket.
///
/// They will also silently ignore any interfaces that are not up, if there is
/// no IPv4 Address on the NIC, if we receive a packet from an IPv6 address, or
/// finally if the broadcast packet is not a MION Identity response.
pub async fn discover_bridges(
	fetch_detailed_info: bool,
) -> Result<UnboundedReceiver<MionIdentity>, CatBridgeError> {
	discover_bridges_with_logging_hooks(fetch_detailed_info, noop_logger_interface).await
}

/// Discover all the Cat-Dev Bridges actively on the network.
///
/// This is the function that allows you to specify EXTRA logging hooks (e.g.
/// those that aren't written to [`tracing`], for like when you need to manually
/// recreate a CLI with old hacky `println!`).
///
/// You probably want [`discover_bridges`].
///
/// ## Errors
///
/// See the error notes for [`discover_bridges`].
pub async fn discover_bridges_with_logging_hooks<InterfaceLoggingHook>(
	fetch_detailed_info: bool,
	interface_logging_hook: InterfaceLoggingHook,
) -> Result<UnboundedReceiver<MionIdentity>, CatBridgeError>
where
	InterfaceLoggingHook: Fn(&'_ Addr) + Clone + Send + 'static,
{
	let to_broadcast = Bytes::from(MionIdentityAnnouncement::new(fetch_detailed_info));
	let mut tasks = JoinSet::new();

	for (interface_addr, interface_ipv4) in get_all_broadcast_addresses()? {
		let broadcast_messaged_cloned = to_broadcast.clone();
		let cloned_iface_hook = interface_logging_hook.clone();
		tasks
			.build_task()
			.name(&format!("cat_dev::discover_mion::{interface_ipv4}"))
			.spawn(async move {
				broadcast_to_mions_on_interface(
					broadcast_messaged_cloned,
					interface_addr,
					interface_ipv4,
					cloned_iface_hook,
				)
				.await
			})
			.map_err(|_| CatBridgeError::SpawnFailure)?;
	}

	let mut listening_sockets = Vec::with_capacity(tasks.len());
	while let Some(joined) = tasks.join_next().await {
		let joined_result = match joined {
			Ok(data) => data,
			Err(cause) => {
				tasks.abort_all();
				return Err(CatBridgeError::JoinFailure(cause));
			}
		};
		let mut opt_socket = match joined_result {
			Ok(optional_socket) => optional_socket,
			Err(cause) => {
				tasks.abort_all();
				return Err(cause);
			}
		};
		if let Some(socket) = opt_socket.take() {
			listening_sockets.push(socket);
		}
	}

	let mut our_addresses = FnvHashSet::with_capacity_and_hasher(
		listening_sockets.len(),
		BuildHasherDefault::default(),
	);
	for sock in &listening_sockets {
		if let Ok(our_addr) = sock.local_addr() {
			our_addresses.insert(our_addr.ip());
		}
	}

	let streams = listening_sockets
		.into_iter()
		.map(|socket| Box::pin(unfold(socket, unfold_socket)))
		.collect::<Vec<_>>();
	// Combine every single socket receive into a single receive stream.
	let mut single_stream = futures::stream::select_all(streams);
	let timeout_at = Instant::now() + Duration::from_secs(MION_ANNOUNCE_TIMEOUT_SECONDS);
	let (send, recv) = unbounded_channel::<MionIdentity>();

	tokio::task::spawn(async move {
		loop {
			tokio::select! {
				opt = single_stream.next() => {
					let Some((read_data_len, from, mut buff)) = opt else {
						continue;
					};
					buff.truncate(read_data_len);
					let frozen = buff.freeze();

					let from_ip = from.ip();
					if our_addresses.contains(&from_ip) {
						debug!("broadcast saw our own message");
						continue;
					}
					let ip_address = match from_ip {
						IpAddr::V4(v4) => v4,
						IpAddr::V6(v6) => {
							debug!(%v6, "broadcast packet from IPv6, ignoring, can't be announcement");
							continue;
						},
					};

					let Ok(identity) = MionIdentity::try_from((ip_address, frozen.clone())) else {
						warn!(%from, packet = %format!("{frozen:02x?}"), "could not parse packet from MION");
						continue;
					};
					if let Err(_closed) = send.send(identity) {
						break;
					}
				}
				() = tokio::time::sleep_until(timeout_at) => {
					break;
				}
			}
		}
	});

	Ok(recv)
}

/// Attempt to find a specific MION by searching for a specific field.
///
/// This _may_ cause a full discovery search to run, or may send a direct
/// packet to the device itself.
///
/// ## Errors
///
/// - If we fail to spawn a task to concurrently look up the MIONs, and we need
///   to do a full discovery search.
/// - If any task fails to create a socket, and broadcast on that socket.
pub async fn find_mion(
	find_by: MIONFindBy,
	find_detailed: bool,
	early_scan_timeout: Option<Duration>,
) -> Result<Option<MionIdentity>, CatBridgeError> {
	find_mion_with_logging_hooks(
		find_by,
		find_detailed,
		early_scan_timeout,
		noop_logger_interface,
	)
	.await
}

/// Attempt to find a specific MION by searching for a specific field.
///
/// This _may_ cause a full discovery search to run, or may send a packet
/// directly to the device itself.
///
/// You probably want [`find_mion`] without logging hooks. Again logs still get
/// generated through the [`tracing`] crate. This is purely for those who need some
/// extra manual logging, say because you're implementing a broken CLI.
///
/// It should also be noted YOU MAY NOT get logging callbacks, if we don't
/// need to do a full scan. You can call [`MIONFindBy::will_cause_full_scan`]
/// in order to determine if you'll get logging callbacks.
///
/// ## Errors
///
/// - If we fail to spawn a task to concurrently look up the MIONs, and we need
///   to do a full discovery search.
/// - If any task fails to create a socket, and broadcast on that socket.
pub async fn find_mion_with_logging_hooks<InterfaceLoggingHook>(
	find_by: MIONFindBy,
	find_detailed_info: bool,
	early_scan_timeout: Option<Duration>,
	interface_logging_hook: InterfaceLoggingHook,
) -> Result<Option<MionIdentity>, CatBridgeError>
where
	InterfaceLoggingHook: Fn(&'_ Addr) + Clone + Send + 'static,
{
	let (find_by_mac, find_by_name) = match find_by {
		MIONFindBy::Ip(ipv4) => {
			let local_socket =
				UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MION_CONTROL_PORT))
					.await
					.map_err(|_| NetworkError::BindAddressError)?;
			local_socket
				.connect(SocketAddrV4::new(ipv4, MION_CONTROL_PORT))
				.await
				.map_err(NetworkError::IOError)?;
			local_socket
				.send(&Bytes::from(MionIdentityAnnouncement::new(
					find_detailed_info,
				)))
				.await
				.map_err(NetworkError::IOError)?;

			let mut buff = BytesMut::zeroed(8192);
			tokio::select! {
				result = local_socket.recv(&mut buff) => {
					let actual_size = result.map_err(NetworkError::IOError)?;
					buff.truncate(actual_size);
				}
				() = sleep(Duration::from_secs(MION_ANNOUNCE_TIMEOUT_SECONDS)) => {
					return Ok(None);
				}
			}
			return Ok(Some(MionIdentity::try_from((ipv4, buff.freeze()))?));
		}
		MIONFindBy::MacAddress(mac) => (Some(mac), None),
		MIONFindBy::Name(name) => (None, Some(name)),
	};

	let mut recv_channel =
		discover_bridges_with_logging_hooks(find_detailed_info, interface_logging_hook).await?;
	loop {
		tokio::select! {
			opt = recv_channel.recv() => {
				let Some(identity) = opt else {
					// No more identities being received.
					break;
				};

				if let Some(filter_mac) = find_by_mac.as_ref() {
					if *filter_mac == identity.mac_address() {
						return Ok(Some(identity));
					}
				}
				if let Some(filter_name) = find_by_name.as_ref() {
					if filter_name == identity.name() {
						return Ok(Some(identity));
					}
				}
			}
			() = sleep(early_scan_timeout.unwrap_or(Duration::from_secs(MION_ANNOUNCE_TIMEOUT_SECONDS * 2))) => {
				break;
			}
		}
	}

	Ok(None)
}

/// A way to search for a single MION board.
///
/// Some of these can end up causing a full discovery broadcast, some of them
/// cause just a single packet to a single ip address. You can parse these from
/// a string or from one of the associated types.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum MIONFindBy {
	/// Search by a specific IP Address.
	///
	/// The IP Address has to be a V4 address, as the MIONs do not actually
	/// support being on an IPv6 address, and using DHCPv6.
	///
	/// This searching type will only send a specific request to the specific
	/// IPv4 address that you've specified here. IT WILL NOT cause a full
	/// broadcast to happen.
	Ip(Ipv4Addr),
	/// Search by a mac address coming from a specific device.
	///
	/// This searching type will cause a FULL Broadcast to happen. Meaning we
	/// will receive potentially many mac addresses that we have to ignore. We
	/// could in theory avoid this by using RARP (aka reverse arp) requests.
	/// However, that requires running as an administrator on many OS's to issue
	/// full RARP's requests. In theory we could parse things like
	/// `/proc/net/arp`, but that requires doing things like sending
	/// pings/broadcasts first which isn't always possible, especially because we
	/// don't know the IP Address before hand.
	///
	/// Maybe one day it would be possible to use RARP requests.
	MacAddress(MacAddress),
	/// Search by the name of a Cat-Dev Bridge.
	///
	/// This searching t ype will cause a FULL Broadcast to happen. Meaning we
	/// will receive potentially many broadcast responses that we might have to
	/// ignore. There isn't really a way to avoid this without keeping a cache
	/// somewhere. In theory a user could still this, and just pass in find by
	/// ip with their own cache.
	Name(String),
}
impl MIONFindBy {
	/// Techincally the name can collide with a mac address, and even techincally
	/// an IP.
	///
	/// To help provide similar APIs to the CLIs we offer
	/// `MIONFindBy::Name(value)`, and `MIONFindBy::from_name_or_ip(value)`,
	/// and finally `MIONFindBy::from(value)` to let you choose which collisions
	/// if any you're okay with.
	#[must_use]
	pub fn from_name_or_ip(value: String) -> Self {
		if let Ok(ipv4) = value.as_str().parse::<Ipv4Addr>() {
			Self::Ip(ipv4)
		} else {
			Self::Name(value)
		}
	}

	/// Determine if the scanning method you're actively using will cause a full
	/// scan of the network.
	#[must_use]
	pub const fn will_cause_full_scan(&self) -> bool {
		match self {
			Self::Ip(ref _ip) => false,
			Self::MacAddress(ref _mac) => true,
			Self::Name(ref _name) => true,
		}
	}
}
impl From<String> for MIONFindBy {
	fn from(value: String) -> Self {
		// First we try parsing a mac address, then an ipv4, before we just give up
		// and use a name.
		if let Ok(mac) = MacAddress::try_from(value.as_str()) {
			Self::MacAddress(mac)
		} else {
			Self::from_name_or_ip(value)
		}
	}
}
impl Display for MIONFindBy {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> FmtResult {
		match self {
			Self::Ip(ref ip) => write!(fmt, "{ip}"),
			Self::MacAddress(ref mac) => write!(fmt, "{mac}"),
			Self::Name(ref name) => write!(fmt, "{name}"),
		}
	}
}

/// Get a list of all the network interfaces to actively scanning on.
///
/// NOTE: this doesn't actually fetch all the broadcast addresses, just the
/// potential ones we MIGHT be able to scan on. This is specifically required
/// for implementing the broken behavior of `findbridge`, which DOES NOT
/// actually ensure that a broadcast address could be made at all.
///
/// Thanks `findbridge`.
///
/// ## Errors
///
/// - If we cannot list all the network interfaces present on the system.
pub fn get_all_broadcast_addresses() -> Result<Vec<(Addr, Ipv4Addr)>, CatBridgeError> {
	Ok(NetworkInterface::show()
		.map_err(|cause| {
			error!(?cause, "could not list network interfaces on this device");
			CatBridgeError::NetworkError(NetworkError::ListInterfacesError)
		})?
		.into_iter()
		.fold(Vec::<(Addr, Ipv4Addr)>::new(), |mut accum, iface| {
			for local_address in &iface.addr {
				let ip = match local_address.ip() {
					IpAddr::V4(ref v4) => {
						if !v4.is_private() && !v4.is_link_local() {
							debug!(?iface, ?local_address, "will not broadcast to public ips");
							continue;
						}

						*v4
					}
					IpAddr::V6(_) => {
						debug!(?iface, ?local_address, "cannot broadcast to IPv6 addresses");
						continue;
					}
				};

				accum.push((*local_address, ip));
			}

			accum
		}))
}

/// Broadcast to all the MIONs on a particular network interface.
///
/// This doesn't actually read the values (we want to queue up all the reads
/// so we can read from them all concurrently with a timeout that applies to
/// all of them).
async fn broadcast_to_mions_on_interface<InterfaceLoggingHook>(
	body_to_broadcast: Bytes,
	interface_addr: Addr,
	interface_ipv4: Ipv4Addr,
	interface_hook: InterfaceLoggingHook,
) -> Result<Option<UdpSocket>, CatBridgeError>
where
	InterfaceLoggingHook: Fn(&'_ Addr),
{
	// Nintendo just blindly prints this even if there is no broadcast address
	// and IT WILL fail.
	interface_hook(&interface_addr);
	let Some(broadcast_address) = interface_addr.broadcast() else {
		debug!(
			?interface_addr,
			?interface_ipv4,
			"failed to get broadcast address"
		);
		return Ok(None);
	};

	debug!(
		?interface_addr,
		?interface_ipv4,
		"actually broadcasting to interface"
	);

	let local_socket = UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(
		interface_ipv4,
		MION_CONTROL_PORT,
	)))
	.await
	.map_err(|_| NetworkError::BindAddressError)?;
	local_socket
		.set_broadcast(true)
		.map_err(|_| NetworkError::SetBroadcastFailure)?;
	local_socket
		.send_to(
			&body_to_broadcast,
			SocketAddr::new(broadcast_address, MION_CONTROL_PORT),
		)
		.await
		.map_err(NetworkError::IOError)?;
	Ok(Some(local_socket))
}

/// Unfold sockets goal is to turn reading from a socket over & over into a
/// stream.
///
/// When the stream has produced a value, and gets polled again,
/// it queues up another read, and so on.
async fn unfold_socket(sock: UdpSocket) -> Option<((usize, SocketAddr, BytesMut), UdpSocket)> {
	let mut buff = BytesMut::zeroed(1024);
	let Ok((len, addr)) = sock.recv_from(&mut buff).await else {
		warn!("failed to receive data from broadcast socket");
		return None;
	};
	Some(((len, addr, buff), sock))
}

/// A logger to use when we don't have another logger passed in.
#[inline]
fn noop_logger_interface(_: &Addr) {}

#[cfg(test)]
mod unit_tests {
	use super::*;

	#[test]
	pub fn can_list_at_least_one_interface() {
		assert!(
			!get_all_broadcast_addresses().expect("Failed to list all broadcast addresses!").is_empty(),
			"Failed to list all broadcast addresses... for some reason your PC isn't compatible to scan devices... perhaps you don't have a private IPv4 address?",
		);
	}

	/// Although we can't actually scan for a real device, as not everyone will
	/// have that device on their network.
	///
	/// However, we can scan for a device that we know is guaranteed to not
	/// exist, so we look for a device with a name that is non-ascii, as device
	/// names have to be ascii.
	#[tokio::test]
	pub async fn cant_find_nonexisting_device() {
		assert!(
			find_mion(MIONFindBy::Name("𩸽".to_owned()), false, None)
				.await
				.expect("Failed to scan to find a specific mion")
				.is_none(),
			"Somehow found a MION that can't exist?"
		);
		assert!(
			find_mion(MIONFindBy::Name("𩸽".to_owned()), true, None)
				.await
				.expect("Failed to scan to find a specific mion")
				.is_none(),
			"Somehow found a MION that can't exist?"
		);
		assert!(
			find_mion(
				MIONFindBy::Name("𩸽".to_owned()),
				true,
				Some(Duration::from_secs(3))
			)
			.await
			.expect("Failed to scan to find a specific mion")
			.is_none(),
			"Somehow found a MION that can't exist?"
		);
	}
}
