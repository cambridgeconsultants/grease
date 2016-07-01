extern crate cuslip;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate time;

use std::env;
use std::thread;
use std::time::Duration;

use cuslip::{RequestSendable, socket};
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

    let msg = socket::ReqBind { addr: "0.0.0.0:8000".parse().unwrap() };

    let (tx, rx) = cuslip::make_channel();

    socket_thread.send(msg.wrap(&tx)).unwrap();

    let cfm = rx.recv().unwrap();

    info!("Got cfm: {:?}", cfm);

    info!("Sleeping for 30 seconds...");

    thread::sleep(Duration::new(30, 0));

    info!("Bye!");
}
