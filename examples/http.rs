//! # http - a grease example using http and sockets

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
use grease::http;
use grease::{Context, IndicationTask, Message, MessageReceiver};

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

	info!("Hello, this is the grease HTTP example.");
	info!("Running HTTP server on {}", bind_addr);

	let socket_thread = socket::make_task();
	let http_thread = http::make_task(&socket_thread);
	let (tx, rx) = grease::make_channel();

	{
		let bind_req = http::ReqBind {
			addr: bind_addr,
			context: Context::default(),
		};
		http_thread.send_request(bind_req, &tx);
	}

	let mut n: Context = Context::default();


	for msg in rx.iter() {
		MessageReceiver::render(&msg);
		match msg {
			Message::Indication(IndicationTask::Http(http::Indication::RxRequest(ref ind))) => {
				let body_msg = format!("This is test {}\r\n", n);
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
				http_thread.send_request(start, &tx);
				let body = http::ReqResponseBody {
					handle: ind.connection_handle,
					context: ctx,
					data: body_msg.into_bytes(),
				};
				http_thread.send_request(body, &tx);
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
