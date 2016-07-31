//! # cuslip - example application

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

extern crate cuslip;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate time;

use std::env;
use std::thread;

use cuslip::{http, socket};
use env_logger::LogBuilder;
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
    builder.format(format).filter(None, LogLevelFilter::Info);
    if env::var("RUST_LOG").is_ok() {
        // Allow environment variable override
        builder.parse(&env::var("RUST_LOG").unwrap());
    }
    builder.init().unwrap();

    info!("Hello, this is cuslip (pronounced copper-slip).");
    info!("It's what you put on threads when you have rust issues...");

    let socket_thread = socket::make_task();
    let http_thread = http::make_task(&socket_thread);
    let (tx, rx) = cuslip::make_channel();

    {
        let bind_req = http::ReqBind {
            context: 1,
            addr: "0.0.0.0:8000".parse().unwrap(),
        };
        http_thread.send_request(bind_req, &tx);
        debug!("Got cfm for 8000 HTTP bind: {:?}", rx.recv().unwrap());
    }

    {
        let bind_req = socket::ReqBind {
            context: 2,
            addr: "0.0.0.0:8001".parse().unwrap(),
        };
        socket_thread.send_request(bind_req, &tx);
        debug!("Got cfm for 8001 raw socket bind: {:?}", rx.recv().unwrap());
    }

    let mut n: cuslip::Context = 0;

    loop {
        debug!("Sleeping...");
        let ind = rx.recv().unwrap();
        if let cuslip::Message::Indication(
            cuslip::Indication::Socket(
                socket::SocketInd::Received(ref ind_rcv))) = ind {
            info!("Got {} bytes of input", ind_rcv.data.len());
            let recv_rsp = socket::RspReceived { handle: ind_rcv.handle };
            socket_thread.send_nonrequest(recv_rsp);
            let reply_data = ind_rcv.data.clone();
            let send_req = socket::ReqSend { handle: ind_rcv.handle, data: reply_data, context: n };
            socket_thread.send_request(send_req, &tx);
            n = n + 1;
        }
        drop(ind);
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
    format!("{},{:03} - {:06} - {:10} - {}",
            time::strftime("%Y-%m-%d %H:%M:%S", &ts).unwrap(),
            ts.tm_nsec / 1_000_000,
            record.level(),
            thread_name,
            record.args())
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
