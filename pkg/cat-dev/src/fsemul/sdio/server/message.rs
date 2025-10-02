//! Handle SDIO message requests coming into our server.

use crate::{
	errors::CatBridgeError,
	fsemul::sdio::{
		errors::SDIONetworkError,
		proto::message::{SdioControlMessage, SdioControlMessageRequest},
		server::SDIO_PRINTF_BUFFS,
	},
	net::{additions::StreamID, server::requestable::Body},
};
use bytes::Bytes;
use tracing::{debug, info};

pub(super) async fn handle_message(
	stream_id: StreamID,
	Body(request): Body<SdioControlMessageRequest>,
) -> Result<Option<Bytes>, CatBridgeError> {
	let Some(mut buff) = SDIO_PRINTF_BUFFS.get_async(&stream_id.to_raw()).await else {
		// Should be unreachable, but if a catastrophic error occurs.
		return Err(SDIONetworkError::PrintfMissingBuffer(stream_id.to_raw()).into());
	};

	let mut should_process = false;
	for message in request.messages_owned() {
		match message {
			SdioControlMessage::Printf(_extra, to_print) => {
				buff.push_str(&to_print);
				should_process = true;
			}
			SdioControlMessage::Unknown(buff) => {
				debug!(
					buff = format!("{:02X?}", buff),
					"Unknown message type == 9 for SDIO, Not Sure How to Respond?",
				);
			}
			SdioControlMessage::RecordBootMode(version) => {
				return Ok(Some(Bytes::try_from(SdioControlMessageRequest::new(
					vec![SdioControlMessage::RecordBootMode(version)],
				))?));
			}
		}
	}

	if should_process {
		process_log_messages(&mut buff);
	}

	Ok(None)
}

fn process_log_messages(printf_buff: &mut String) {
	let mut used_one = false;

	loop {
		while let Some(line_ending) = printf_buff.find("\r\n") {
			used_one = true;
			let remaining = printf_buff.split_off(line_ending + 2);
			let actual_line = std::mem::replace(printf_buff, remaining);

			// Ignore empty newlines they try to send.
			if !actual_line.trim().is_empty() {
				info!("{}", actual_line.trim(),);
			}
		}
		while let Some(line_ending) = printf_buff.find('\n') {
			used_one = true;
			let remaining = printf_buff.split_off(line_ending + 1);
			let actual_line = std::mem::replace(printf_buff, remaining);

			// Ignore empty newlines they try to send.
			if !actual_line.trim().is_empty() {
				info!("{}", actual_line.trim(),);
			}
		}
		while let Some(line_ending) = printf_buff.find('\r') {
			used_one = true;
			let remaining = printf_buff.split_off(line_ending + 1);
			let actual_line = std::mem::replace(printf_buff, remaining);

			// Ignore empty newlines they try to send.
			if !actual_line.trim().is_empty() {
				info!("{}", actual_line.trim(),);
			}
		}

		if !used_one {
			break;
		}
		used_one = false;
	}
}
