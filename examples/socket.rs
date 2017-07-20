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
use grease::socket::*;

use log::{LogRecord, LogLevelFilter};

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

	let socket_thread = make_task();
	let (tx, rx) = grease::make_channel();
	{
		let bind_req = ReqBind {
			context: 2,
			addr: bind_addr,
			conn_type: ConnectionType::Stream,
		};
		socket_thread.send_request(bind_req, &tx);
	}

	let mut n: grease::Context = 0;

	for msg in rx.iter() {
		grease::MessageReceiver::render(&msg);
		match msg {
			grease::Message::Indication(grease::Indication::Socket(Indication::Received(ind))) => {
				info!("Echoing {} bytes of input", ind.data.len());
				let recv_rsp = RspReceived { handle: ind.handle };
				socket_thread.send_nonrequest(recv_rsp);
				let send_req = ReqSend {
					handle: ind.handle,
					data: ind.data,
					context: n,
				};
				socket_thread.send_request(send_req, &tx);
				n = n + 1;
			}
			grease::Message::Indication(grease::Indication::Socket(Indication::Connected(ind))) => {
				info!(
					"Got connection from {}, handle = {}",
					ind.peer,
					ind.conn_handle
				);
			}
			grease::Message::Indication(grease::Indication::Socket(Indication::Dropped(ind))) => {
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
