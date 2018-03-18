//! # http - a grease example using http and sockets

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

extern crate env_logger;
extern crate grease;
extern crate grease_http as http;
extern crate grease_socket as socket;
#[macro_use]
extern crate log;
extern crate time;

use std::sync::mpsc;
use std::net;

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

#[derive(Debug)]
enum Incoming {
	SocketConfirm(socket::Confirm),
	SocketIndication(socket::Indication),
	HttpConfirm(http::Confirm),
	HttpIndication(http::Indication),
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

impl grease::ServiceUser<http::Confirm, http::Indication> for Handle {
	fn send_confirm(&self, cfm: http::Confirm) {
		self.chan.send(Incoming::HttpConfirm(cfm)).unwrap();
	}

	fn send_indication(&self, ind: http::Indication) {
		self.chan.send(Incoming::HttpIndication(ind)).unwrap();
	}

	fn clone(&self) -> http::UserHandle {
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

	info!("Hello, this is the grease HTTP example.");
	info!("Running HTTP server on {}", bind_addr);

	let socket_thread = socket::make_task();
	let http_thread = http::make_task(socket_thread);
	let (tx, rx) = mpsc::channel();
	let handle = Handle { chan: tx };

	{
		let bind_req = http::ReqBind {
			addr: bind_addr,
			context: Context::default(),
		};
		http_thread.send_request(http::Request::Bind(bind_req), &handle);
	}

	let mut n: Context = Context::default();


	for msg in rx.iter() {
		debug!("Rx: {:?}", msg);
		match msg {
			Incoming::HttpIndication(http::Indication::RxRequest(ind)) => {
				let body_msg = format!("This is test {}\r\nYou used URL '{}'\r\n", n, ind.url);
				info!("Got HTTP request {:?} {}", ind.method, ind.url);
				let ctx = n.take();
				let start = http::ReqResponseStart {
					status: http::HttpResponseStatus::OK,
					handle: ind.connection_handle,
					context: ctx,
					content_type: String::from("text/plain"),
					length: Some(body_msg.len()),
					headers: http::HeaderMap::new(),
				};
				http_thread.send_request(http::Request::ResponseStart(start), &handle);
				let body = http::ReqResponseBody {
					handle: ind.connection_handle,
					context: ctx,
					data: body_msg.into_bytes(),
				};
				http_thread.send_request(http::Request::ResponseBody(body), &handle);
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
