extern crate cuslip;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate time;

use std::env;
use std::thread;

use cuslip::{RequestSendable, NonRequestSendable, socket};
use env_logger::LogBuilder;
use log::{LogRecord, LogLevelFilter};

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

    let socket_thread = socket::new();
    let (tx, rx) = cuslip::make_channel();

    let msg = socket::ReqBind { addr: "0.0.0.0:8000".parse().unwrap() };
    socket_thread.send(msg.wrap(&tx)).unwrap();
    let cfm = rx.recv().unwrap();
    info!("Got cfm for 8000 bind: {:?}", cfm);

    let msg = socket::ReqBind { addr: "0.0.0.0:8001".parse().unwrap() };
    socket_thread.send(msg.wrap(&tx)).unwrap();
    let cfm = rx.recv().unwrap();
    info!("Got cfm for 8001 bind: {:?}", cfm);

    loop {
        let ind = rx.recv().unwrap();
        info!("Got msg: {:?}", ind);
        if let cuslip::Message::Indication(cuslip::Indication::Socket(socket::SocketInd::Received(x))) =
               ind {
            let rsp = socket::RspReceived { handle: x.handle };
            socket_thread.send(rsp.wrap()).unwrap();
        }
    }
}
