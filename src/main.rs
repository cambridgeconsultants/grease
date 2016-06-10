extern crate cuslip;

fn test_message() {
    let m = cuslip::Message {
        sender: "Test".into(),
        msg_type:
            cuslip::MessageBody::Request(cuslip::Request::Socket(cuslip::socket::SocketRequest::Open {
            addr: "127.0.0.1".into(),
            port: 8000,
        })),
    };
    println!("Sending {:?}", m);
}

fn main() {
    println!("Hello, this is cuslip (pronounced copper-slip).");
    println!("It's what you put on threads when you have rust issues...");

    test_message();

    // Start socket thread
    // Start AT thread
    // Join threads together
}
