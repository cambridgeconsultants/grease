//! # http - a grease example using http and sockets

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

extern crate env_logger;
#[macro_use]
extern crate grease;
extern crate grease_http as http;
extern crate grease_socket as socket;
#[macro_use]
extern crate log;

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

struct Handle(mpsc::Sender<Incoming>);

app_map! {
	generate: Incoming,
	handle: Handle,
	used: {
		http: (HttpCfm, HttpInd)
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
	let handle = Handle(tx);

	http_thread.send_request(
		http::ReqBind {
			addr: bind_addr,
			context: Context::default(),
		}.into(),
		&handle,
	);

	let mut n: Context = Context::default();

	for msg in rx.iter() {
		match msg {
			Incoming::HttpInd(http::Indication::RxRequest(ind)) => {
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
				http_thread.send_request(start.into(), &handle);
				let body = http::ReqResponseBody {
					handle: ind.connection_handle,
					context: ctx,
					data: body_msg.into_bytes(),
				};
				http_thread.send_request(body.into(), &handle);
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
