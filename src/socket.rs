//! # cuslip Sockets
//!
//! The cuslip socket thread makes handling sockets easier. You request the
//! thread to open a new socket and it does so (if possible). Your thread then
//! receives asynchronous indications when data arrives on the socket.

#![allow(dead_code)]

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use std::thread;
use super::{MessageRequest, Message, MessageSender, MessageConfirmation};

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Requests that can be sent to the Socket task
/// We box all the parameters, in case any of the structs are large as we don't
/// want to bloat the master Message type.
pub enum SocketReq {
    /// Open a socket
    Open(Box<SocketReqOpen>),
    /// Close an open socket
    Close(Box<SocketReqClose>),
    /// Send something on a socket
    Send(Box<SocketReqSend>),
}

/// Opens a new socket.
pub struct SocketReqOpen {
    /// The address to bind (e.g. "192.168.0.1")
    addr: String,
    /// The TCP port to listen on (e.g. 8000)
    port: u16,
}

/// Closes a socket
pub struct SocketReqClose {
    /// The handle from a SocketCfmOpen
    handle: SocketHandle,
}

/// Sends data on a socket
pub struct SocketReqSend {
    /// The handle from a SocketCfmOpen
    handle: SocketHandle,
    /// The data to be sent
    /// TODO should this be a u8 array of some sort?
    msg: String,
}

/// Confirmations sent from the Socket task in answer to a SocketReq
pub enum SocketCfm {
    // Opened a socket
    Open(Box<SocketCfmOpen>),
    // Closed an open socket
    Close(Box<SocketCloseCfm>),
    // Sent something on a socket
    Send(Box<SocketSendCfm>),
}

/// Reply to a SocketReqOpen.
pub struct SocketCfmOpen {
    result: Result<SocketHandle, SocketError>,
}

/// Reply to a SocketReqClose.
pub struct SocketCloseCfm {
    handle: SocketHandle,
    result: Result<(), SocketError>,
}

/// Reply to a SocketReqSend. The data has not necessarily
/// been sent, but it is safe to send some more data.
pub struct SocketSendCfm {
    handle: SocketHandle,
    result: Result<(), SocketError>,
}

/// Asynchronous indications sent by the Socket task
pub enum SocketInd {
    // No more socket
    Dropped(Box<SocketIndDropped>),
    // Data arrived
    Received(Box<SocketIndReceived>),
}

/// Indicates that a socket has been dropped.
pub struct SocketIndDropped {
    handle: SocketHandle, // reason?
}

/// Indicates that data has arrived on the socket
pub struct SocketIndReceived {
    handle: SocketHandle,
    data: String,
}

/// Uniquely identifies an open socket
pub type SocketHandle = u32;

/// All possible errors the Socket task might want to
/// report.
#[derive(Debug)]
pub enum SocketError {
    BadAddress,
    Timeout,
    Unknown,
}

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

/// Creates a new socket thread. Returns an object that can be used
/// to send this thead messages.
pub fn make_thread() -> super::MessageSender {
    let (sender, receiver) = super::make_channel();
    thread::spawn(move || main_loop(receiver));
    return sender;
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

fn main_loop(rx: super::MessageReceiver) {
    loop {
        let msg = rx.recv().unwrap();
        match msg {
            Message::Request(reply_to, MessageRequest::Socket(x)) => handle_req(&x, &reply_to),
            _ => panic!("Bad message!"),
        }
    }
}

/// Handle requests
fn handle_req(msg: &SocketReq, reply_to: &MessageSender) {
    println!("Got a socket request message!");
    match *msg {
        SocketReq::Open(ref x) => handle_open(&x, reply_to),
        SocketReq::Close(ref x) => handle_close(&x, reply_to),
        SocketReq::Send(ref x) => handle_send(&x, reply_to),
    }
}

/// Handle a SocketReqOpen.
/// Creates a TcpListener then starts a thread to watch
/// for connections. Each connection then starts a new thread.
/// These threads use a clone of the reply_to field.
fn handle_open(msg: &SocketReqOpen, reply_to: &MessageSender) {
    println!("Got a socket open request. addr={}, port={}",
             msg.addr,
             msg.port);
    let cfm = SocketCfmOpen { result: Err(SocketError::Unknown) };
    let cfm =
        Message::Confirmation(MessageConfirmation::Socket(SocketCfm::Open(Box::new(cfm))));
    reply_to.send(cfm).expect("Couldn't send message");
}

/// Handle a SocketReqClose.
fn handle_close(msg: &SocketReqClose, reply_to: &MessageSender) {
    println!("Got a socket close request. handle={}", msg.handle);
    let cfm = SocketCloseCfm {
        result: Err(SocketError::Unknown),
        handle: msg.handle,
    };
    let cfm = Message::Confirmation(
        MessageConfirmation::Socket(
            SocketCfm::Close(
                Box::new(cfm)
            )
        )
    );
    reply_to.send(cfm).expect("Couldn't send message");
}

/// Handle a SocketReqSend
fn handle_send(msg: &SocketReqSend, reply_to: &MessageSender) {
    println!("Got a socket send request. handle={}", msg.handle);
    let cfm = SocketSendCfm {
        result: Err(SocketError::Unknown),
        handle: msg.handle,
    };
    let cfm =
        Message::Confirmation(MessageConfirmation::Socket(SocketCfm::Send(Box::new(cfm))));
    reply_to.send(cfm).expect("Couldn't send message");
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
