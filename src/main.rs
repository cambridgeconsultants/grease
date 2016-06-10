extern crate cuslip;

fn open_socket() {
    let m = cuslip::Message {
        sender: "Test".into(),
        msg_type:
            cuslip::MessageBody::Request(cuslip::Request::Socket(cuslip::SocketRequest::Open {
            addr: "127.0.0.1".into(),
            port: 8000,
        })),
    };
    println!("Sending {:?}", m);
}

fn main() {
    println!("Hello, this is cuslip (pronounced copper-slip).");
    println!("It's what you put on threads when you have rust issues...");

    open_socket();

}
