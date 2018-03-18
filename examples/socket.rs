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
extern crate time;

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

	fn clone(&self) -> socket::UserHandle {
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

	let socket_thread = socket::make_task();
	let (tx, rx) = mpsc::channel();
	let handle = Handle { chan: tx };
	{
		let bind_req = socket::Request::Bind(socket::ReqBind {
			context: Context::default(),
			addr: bind_addr,
			conn_type: socket::ConnectionType::Stream,
		});
		socket_thread.send_request(bind_req, &handle);
	}

	let mut n: Context = Context::default();

	for msg in rx.iter() {
		match msg {
			Incoming::SocketIndication(socket::Indication::Received(ind)) => {
				let recv_rsp =
					socket::Response::Received(socket::RspReceived { handle: ind.handle });
				socket_thread.send_response(recv_rsp);
				info!("Echoing {} bytes of input", ind.data.len());
				let send_req = socket::Request::Send(socket::ReqSend {
					handle: ind.handle,
					data: ind.data,
					context: n.take(),
				});
				socket_thread.send_request(send_req, &handle);
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
