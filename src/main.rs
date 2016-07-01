extern crate cuslip;

use cuslip::socket;
use cuslip::RequestSendable;

fn main() {
    println!("Hello, this is cuslip (pronounced copper-slip).");
    println!("It's what you put on threads when you have rust issues...");

    let socket_thread = socket::make_thread();

    let msg = socket::SocketReqOpen {
        addr: "0.0.0.0".to_owned(),
        port: 80,
    };

    let (tx, rx) = cuslip::make_channel();

    socket_thread.send(msg.wrap(tx)).unwrap();

    let cfm = rx.recv().unwrap();

    println!("Got cfm: {:?}", cfm);
}
