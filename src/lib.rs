#![allow(dead_code)]

pub mod atcommands;
pub mod socket;

#[derive(Debug)]
pub struct Message {
    // Some identifier from the sender
    pub sender: String,
    // What the sender wishes to send
    pub msg_type: MessageBody,
}

#[derive(Debug)]
pub enum MessageBody {
    Request(Request),
    Confirmation(Confirmation),
    Indication(Indication),
}

#[derive(Debug)]
pub enum Request {
    Socket(socket::SocketRequest),
    AT(atcommands::ATRequest),
}

#[derive(Debug)]
pub enum Confirmation {
    Socket(socket::SocketConfirmation),
    AT(atcommands::ATConfirmation),
}

#[derive(Debug)]
pub enum Indication {
    Socket(socket::SocketIndication),
    AT(atcommands::ATIndication),
}

#[cfg(test)]
mod test {
    #[test]
    fn it_works() {}
}
