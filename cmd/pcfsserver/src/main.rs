use cat_dev::fsemul::{HostFilesystem, pcfs::sata::server::pcfs_sata_server};
use std::{net::Ipv4Addr, str::FromStr};
use tokio::runtime::Runtime;

fn main() {
	let Ok(runtime) = Runtime::new() else {
		println!("TODO(mythra): thread pool spin up failure");
		return;
	};
	runtime.block_on(serve());
}

async fn serve() {
	let fs = HostFilesystem::from_cafe_dir(None)
		.await
		.expect("TODO HOSTFS FAILURE");
	let server = pcfs_sata_server(
		fs,
		std::env::args()
			.next()
			.and_then(|ipstr| Ipv4Addr::from_str(&ipstr).ok()),
		None,
		false,
		false,
		false,
		None,
		None,
		true,
		None,
		true,
		true,
	)
	.await
	.expect("TODO failed to spin up server");
	println!(
		"TODO mythra debug: server on: {}:{}",
		server.ip(),
		server.port()
	);
	server.bind().await.expect("TODO Failed to serve server");
}
