//! # cuslip Sockets
//!
//! The cuslip socket thread makes handling sockets easier. A user requests the
//! socket task opens a new socket and it does so (if possible). The user then
//! receives asynchronous indications when data arrives on the socket and/or
//! when the socket closes.

#![allow(dead_code)]

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use std::collections::HashMap;
use super::{NonRequestSendable, RequestSendable};

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Requests that can be sent to the Socket task
/// We box all the parameters, in case any of the structs are large as we don't
/// want to bloat the mastersuper::Message super::type.
#[derive(Debug)]
pub enum SocketReq {
    /// Open a socket
    Open(Box<SocketReqOpen>),
    /// Close an open socket
    Close(Box<SocketReqClose>),
    /// Send something on a socket
    Send(Box<SocketReqSend>),
}

/// Opens a new socket.
#[derive(Debug)]
pub struct SocketReqOpen {
    /// The address to bind (e.g. "192.168.0.1")
    pub addr: String,
    /// The TCP port to listen on (e.g. 8000)
    pub port: u16,
}

/// Make SocketReqOpen sendable over a channel
impl super::RequestSendable for SocketReqOpen {
    fn wrap(self, reply_to: &super::MessageSender) -> super::Message {
        super::Message::Request(reply_to.clone(),
                                super::Request::Socket(SocketReq::Open(Box::new(self))))
    }
}

/// Closes a socket
#[derive(Debug)]
pub struct SocketReqClose {
    /// The handle from a SocketCfmOpen
    pub handle: SocketHandle,
}

/// Make SocketReqClose sendable over a channel
impl super::RequestSendable for SocketReqClose {
    fn wrap(self, reply_to: &super::MessageSender) -> super::Message {
        super::Message::Request(reply_to.clone(),
                                super::Request::Socket(SocketReq::Close(Box::new(self))))
    }
}

/// Sends data on a socket
#[derive(Debug)]
pub struct SocketReqSend {
    /// The handle from a SocketCfmOpen
    pub handle: SocketHandle,
    /// The data to be sent
    /// TODO should this be a u8 array of some sort?
    pub msg: String,
}

/// Make SocketReqSend sendable over a channel
impl super::RequestSendable for SocketReqSend {
    fn wrap(self, reply_to: &super::MessageSender) -> super::Message {
        super::Message::Request(reply_to.clone(),
                                super::Request::Socket(SocketReq::Send(Box::new(self))))
    }
}

/// Confirmations sent from the Socket task in answer to a SocketReq
#[derive(Debug)]
pub enum SocketCfm {
    // Opened a socket
    Open(Box<SocketCfmOpen>),
    // Closed an open socket
    Close(Box<SocketCfmClose>),
    // Sent something on a socket
    Send(Box<SocketSendCfm>),
}

/// Reply to a SocketReqOpen.
#[derive(Debug)]
pub struct SocketCfmOpen {
    pub result: Result<SocketHandle, SocketError>,
}

/// Make SocketCfmOpen sendable over a channel
impl super::NonRequestSendable for SocketCfmOpen {
    fn wrap(self) -> super::Message {
        super::Message::Confirmation(super::Confirmation::Socket(SocketCfm::Open(Box::new(self))))
    }
}

/// Reply to a SocketReqClose.
#[derive(Debug)]
pub struct SocketCfmClose {
    pub handle: SocketHandle,
    pub result: Result<(), SocketError>,
}

/// Make SocketCfmClose sendable over a channel
impl super::NonRequestSendable for SocketCfmClose {
    fn wrap(self) -> super::Message {
        super::Message::Confirmation(super::Confirmation::Socket(SocketCfm::Close(Box::new(self))))
    }
}

/// Reply to a SocketReqSend. The data has not necessarily
/// been sent, but it is safe to send some more data.
#[derive(Debug)]
pub struct SocketSendCfm {
    pub handle: SocketHandle,
    pub result: Result<(), SocketError>,
}

/// Make SocketSendCfm sendable over a channel
impl super::NonRequestSendable for SocketSendCfm {
    fn wrap(self) -> super::Message {
        super::Message::Confirmation(super::Confirmation::Socket(SocketCfm::Send(Box::new(self))))
    }
}

/// Asynchronous indications sent by the Socket task
#[derive(Debug)]
pub enum SocketInd {
    // No more socket
    Dropped(Box<SocketIndDropped>),
    // Data arrived
    Received(Box<SocketIndReceived>),
}

/// Indicates that a socket has been dropped.
#[derive(Debug)]
pub struct SocketIndDropped {
    pub handle: SocketHandle, // reason?
}

/// Indicates that data has arrived on the socket
#[derive(Debug)]
pub struct SocketIndReceived {
    pub handle: SocketHandle,
    pub data: String,
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

struct ListeningSocket {
    addr: String,
    port: u16,
}

struct ConnectedSocket {
    addr: String,
    port: u16,
}

struct TaskData {
    listeners: HashMap<SocketHandle, ListeningSocket>,
    connections: HashMap<SocketHandle, ConnectedSocket>,
    last_handle: SocketHandle,
}

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
pub fn new() -> super::MessageSender {
    super::make_task("socket", main_loop)
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

/// The task runs this main loop indefinitely.
fn main_loop(rx: super::MessageReceiver) {
    let mut t = TaskData::new();
    loop {
        let msg = rx.recv().unwrap();
        match msg {
            // We only handle our own requests
            super::Message::Request(reply_to, super::Request::Socket(x)) => {
                t.handle_req(&x, &reply_to)
            }
            // We don't have any responses
            // We don't expect any Indications or Confirmations
            // Crash if another module has got it
            _ => warn!("Unexpected message in socket task: {:?}", msg),
        }
    }
}

impl TaskData {
    /// Init the context
    pub fn new() -> TaskData {
        TaskData {
            listeners: HashMap::new(),
            connections: HashMap::new(),
            last_handle: 0,
        }
    }

    /// Handle requests
    pub fn handle_req(&mut self, msg: &SocketReq, reply_to: &super::MessageSender) {
        debug!("Got a socket request message!");
        match *msg {
            SocketReq::Open(ref x) => self.handle_open(&x, reply_to),
            SocketReq::Close(ref x) => self.handle_close(&x, reply_to),
            SocketReq::Send(ref x) => self.handle_send(&x, reply_to),
        }
    }

    /// Open a new socket with the given parameters.
    fn handle_open(&mut self, msg: &SocketReqOpen, reply_to: &super::MessageSender) {
        debug!("Got a socket open request. addr={}, port={}",
               msg.addr,
               msg.port);
        let cfm = SocketCfmOpen { result: Err(SocketError::Unknown) };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
    }

    /// Handle a SocketReqClose.
    fn handle_close(&mut self, msg: &SocketReqClose, reply_to: &super::MessageSender) {
        debug!("Got a socket close request. handle={}", msg.handle);
        let cfm = SocketCfmClose {
            result: Err(SocketError::Unknown),
            handle: msg.handle,
        };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
    }

    /// Handle a SocketReqSend
    fn handle_send(&mut self, msg: &SocketReqSend, reply_to: &super::MessageSender) {
        debug!("Got a socket send request. handle={}", msg.handle);
        let cfm = SocketSendCfm {
            result: Err(SocketError::Unknown),
            handle: msg.handle,
        };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
    }
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
