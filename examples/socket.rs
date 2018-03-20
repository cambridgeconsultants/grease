//! # socket - a grease example using sockets

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

extern crate env_logger;
extern crate grease;
extern crate grease_socket as socket;
#[macro_use]
extern crate log;

use std::net;
use std::sync::mpsc;

use grease::Context;

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

// None

// ****************************************************************************
//
// Private Types
//
// ****************************************************************************

enum Incoming {
	SocketConfirm(socket::Confirm),
	SocketIndication(socket::Indication),
}

struct Handle {
	chan: mpsc::Sender<Incoming>,
}

impl grease::ServiceUser<socket::Confirm, socket::Indication> for Handle {
	fn send_confirm(&self, cfm: socket::Confirm) {
		self.chan.send(Incoming::SocketConfirm(cfm)).unwrap();
	}

	fn send_indication(&self, ind: socket::Indication) {
		self.chan.send(Incoming::SocketIndication(ind)).unwrap();
	}

	fn clone(&self) -> socket::ServiceUserHandle {
		Box::new(Handle {
			chan: self.chan.clone(),
		})
	}
}

// ****************************************************************************
//
// Public Data
//
// ****************************************************************************

// None

// ****************************************************************************
//
// Public Functions
//
// ****************************************************************************

/// Start of our example program
fn main() {
	env_logger::init();
	let bind_addr: net::SocketAddr = "0.0.0.0:8000".parse().unwrap();

	info!("Hello, this is the grease socket example.");
	info!("Running echo server on {}", bind_addr);

	let socket_task = socket::make_task();
	let (tx, rx) = mpsc::channel();
	let handle = Handle { chan: tx };

	socket_task.send_request(
		socket::ReqBind {
			context: Context::default(),
			addr: bind_addr,
			conn_type: socket::ConnectionType::Stream,
		}.into(),
		&handle,
	);

	let mut n: Context = Context::default();

	for msg in rx.iter() {
		match msg {
			Incoming::SocketIndication(socket::Indication::Received(ind)) => {
				socket_task.send_response(socket::RspReceived { handle: ind.handle }.into());
				info!("Echoing {} bytes of input", ind.data.len());
				socket_task.send_request(
					socket::ReqSend {
						handle: ind.handle,
						data: ind.data,
						context: n.take(),
					}.into(),
					&handle,
				);
			}
			Incoming::SocketIndication(socket::Indication::Connected(ind)) => {
				info!(
					"Got connection from {}, handle = {}",
					ind.peer, ind.conn_handle
				);
			}
			Incoming::SocketIndication(socket::Indication::Dropped(ind)) => {
				info!("Connection dropped, handle = {}", ind.handle);
			}
			_ => {}
		}
	}
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

// None

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
