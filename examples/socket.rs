//! # socket - a grease example using sockets

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

extern crate env_logger;
extern crate grease;
#[macro_use]
extern crate log;
extern crate time;

use std::env;
use std::thread;
use std::net;

use env_logger::LogBuilder;
use grease::socket;
use grease::Context;
use std::sync::mpsc;

use socket::ServiceProvider;
use socket::ServiceUser;

use log::{LogLevelFilter, LogRecord};

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

// None

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

enum Incoming {
	SocketConfirm(socket::Confirm),
	SocketIndication(socket::Indication),
}

struct Handle {
	chan: mpsc::Sender<Incoming>,
}

impl socket::ServiceUser for Handle {
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

/// Start of our example program
fn main() {
	// Initialise the logging with a custom logger
	let mut builder = LogBuilder::new();
	builder.format(format).filter(None, LogLevelFilter::Debug);
	if env::var("RUST_LOG").is_ok() {
		// Allow environment variable override
		builder.parse(&env::var("RUST_LOG").unwrap());
	}
	builder.init().unwrap();

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
		socket_thread.send_request(bind_req, handle.clone());
	}

	let mut n: Context = Context::default();

	for msg in rx.iter() {
		match msg {
			Incoming::SocketIndication(socket::Indication::Received(ind)) => {
				std::thread::sleep(std::time::Duration::from_millis(500));
				let recv_rsp =
					socket::Response::Received(socket::RspReceived { handle: ind.handle });
				socket_thread.send_response(recv_rsp);
				info!("Echoing {} bytes of input", ind.data.len());
				let send_req = socket::Request::Send(socket::ReqSend {
					handle: ind.handle,
					data: ind.data,
					context: n.take(),
				});
				socket_thread.send_request(send_req, handle.clone());
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

/// Our custom log function
fn format(record: &LogRecord) -> String {
	let ts = time::now();
	let thread_id = thread::current();
	let thread_name = thread_id.name().unwrap_or("<??>");
	format!(
		"{},{:03} - {:06} - {:10} - {}",
		time::strftime("%Y-%m-%d %H:%M:%S", &ts).unwrap(),
		ts.tm_nsec / 1_000_000,
		record.level(),
		thread_name,
		record.args()
	)
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
