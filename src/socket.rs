//! # cuslip Sockets
//!
//! The cuslip socket task makes handling sockets easier. A user requests the
//! socket task opens a new socket and it does so (if possible). The user then
//! receives asynchronous indications when data arrives on the socket and/or
//! when the socket closes.

#![allow(dead_code)]

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use std::net;
use std::io;
use std::thread;
use std::collections::HashMap;
use mio;
use super::{NonRequestSendable, RequestSendable};

// ****************************************************************************
//
// Public Messages
//
// ****************************************************************************

/// Requests that can be sent to the Socket task
/// We box all the parameters, in case any of the structs are large as we don't
/// want to bloat the master Message type.
#[derive(Debug)]
pub enum SocketReq {
    Bind(Box<ReqBind>),
    Unbind(Box<ReqUnbind>),
    Close(Box<ReqClose>),
    Send(Box<ReqSend>),
}

/// Confirmations sent from the Socket task in answer to a SocketReq
#[derive(Debug)]
pub enum SocketCfm {
    Bind(Box<CfmBind>),
    Unbind(Box<CfmUnbind>),
    Close(Box<CfmClose>),
    Send(Box<CfmSend>),
}

/// Asynchronous indications sent by the Socket task
#[derive(Debug)]
pub enum SocketInd {
    // No more socket
    Connected(Box<IndConnected>),
    // No more socket
    Dropped(Box<IndDropped>),
    // Data arrived
    Received(Box<IndReceived>),
}

/// Bind a listen socket
#[derive(Debug)]
pub struct ReqBind {
    /// The address to bind (e.g. "192.168.0.1")
    pub addr: String,
    /// The TCP port to listen on (e.g. 8000)
    pub port: u16,
}

/// Unbind a bound listen socket
#[derive(Debug)]
pub struct ReqUnbind {
    /// The handle from a CfmBind
    pub handle: OpenHandle,
}

/// Close an open connection
#[derive(Debug)]
pub struct ReqClose {
    /// The handle from a IndConnected
    pub handle: OpenHandle,
}

/// Send something on a connection
#[derive(Debug)]
pub struct ReqSend {
    /// The handle from a CfmBind
    pub handle: OpenHandle,
    /// The data to be sent
    /// TODO should this be a u8 array of some sort?
    pub msg: String,
}

/// Reply to a ReqBind.
#[derive(Debug)]
pub struct CfmBind {
    pub result: Result<ListenHandle, SocketError>,
}

/// Reply to a ReqUnbind.
#[derive(Debug)]
pub struct CfmUnbind {
    /// The handle requested for unbinding
    pub handle: ListenHandle,
    /// Whether we were successful in unbinding
    pub result: Result<(), SocketError>,
}

/// Reply to a ReqClose. Will flush out all
/// existing data.
#[derive(Debug)]
pub struct CfmClose {
    pub handle: OpenHandle,
    pub result: Result<(), SocketError>,
}

/// Reply to a ReqSend. The data has not necessarily
/// been sent, but it is safe to send some more data.
#[derive(Debug)]
pub struct CfmSend {
    pub handle: OpenHandle,
    pub result: Result<(), SocketError>,
}

/// Indicates that a listening socket has been connected to.
#[derive(Debug)]
pub struct IndConnected {
    pub listen_handle: ListenHandle,
    pub open_handle: OpenHandle,
}

/// Indicates that a socket has been dropped.
#[derive(Debug)]
pub struct IndDropped {
    pub handle: OpenHandle,
}

// IndReceived
//

/// Indicates that data has arrived on the socket
#[derive(Debug)]
pub struct IndReceived {
    pub handle: OpenHandle,
    pub data: String,
}

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Uniquely identifies an listening socket
pub type ListenHandle = u32;

/// Uniquely identifies an open socket
pub type OpenHandle = u32;

/// All possible errors the Socket task might want to
/// report.
#[derive(Debug)]
pub enum SocketError {
    IOError(io::Error),
    BadAddress,
    Timeout,
    Unknown,
}

// ****************************************************************************
//
// Private Types
//
// ****************************************************************************

/// Created for every bound (i.e. listening) socket
struct ListeningSocket {
    handle: ListenHandle,
    socket: net::TcpListener,
    thread: thread::JoinHandle<()>,
}

/// Created for every connection receieved on a ListeningSocket
struct ConnectedSocket {
    addr: String,
    port: u16,
}

/// One instance per task. Stores all the task data.
struct TaskContext {
    /// Set of all bound sockets
    listeners: HashMap<ListenHandle, ListeningSocket>,
    /// Set of all connected sockets
    connections: HashMap<OpenHandle, ConnectedSocket>,
    /// The next handle we'll use for a bound socket
    next_listen: ListenHandle,
    /// The next handle we'll use for an open socket
    next_open: OpenHandle,
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

/// Creates a new socket task. Returns an object that can be used
/// to send this task messages.
pub fn new() -> super::MessageSender {
    super::make_task("socket", main_loop)
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

/// The task runs this main loop indefinitely.
/// Unfortunately, to use mio, we have to use their special
/// channels. So, we spin up a thread to bounce from one
/// channel to the other.
fn main_loop(rx: super::MessageReceiver) {
    let mut event_loop = mio::EventLoop::new().unwrap();
    let ch = event_loop.channel();
    let _ = thread::spawn(move || {
        loop {
            let msg = rx.recv().unwrap();
            ch.send(msg).unwrap();
        }
    });
    let mut task_context = TaskContext::new();
    event_loop.run(&mut task_context).unwrap();
}

impl mio::Handler for TaskContext {
    type Timeout = u32;
    type Message = super::Message;
    fn ready(&mut self,
             event_loop: &mut mio::EventLoop<TaskContext>,
             token: mio::Token,
             events: mio::EventSet) {
        warn!("Ready!");
    }
    fn notify(&mut self, event_loop: &mut mio::EventLoop<TaskContext>, msg: super::Message) {
        match msg {
            // We only handle our own requests
            super::Message::Request(reply_to, super::Request::Socket(x)) => {
                self.handle_req(&x, &reply_to)
            }
            // We don't have any responses
            // We don't expect any Indications or Confirmations from other providers
            // If we get here, someone else has made a mistake
            _ => error!("Unexpected message in socket task: {:?}", msg),
        }
    }
    fn timeout(&mut self, event_loop: &mut mio::EventLoop<TaskContext>, timeout: Self::Timeout) {
        warn!("Timeout!");
    }
    fn interrupted(&mut self, event_loop: &mut mio::EventLoop<TaskContext>) {
        warn!("Interrupted!");
    }
    fn tick(&mut self, event_loop: &mut mio::EventLoop<TaskContext>) {
        debug!("Tick!");
    }
}

impl TaskContext {
    /// Init the context
    pub fn new() -> TaskContext {
        TaskContext {
            listeners: HashMap::new(),
            connections: HashMap::new(),
            // It helps the debug to keep these two apart
            next_listen: 0,
            next_open: 1_000_000,
        }
    }

    /// Handle requests
    pub fn handle_req(&mut self, msg: &SocketReq, reply_to: &super::MessageSender) {
        debug!("request: {:?}", msg);
        match *msg {
            SocketReq::Bind(ref x) => self.handle_bind(&x, reply_to),
            SocketReq::Unbind(ref x) => self.handle_unbind(&x, reply_to),
            SocketReq::Close(ref x) => self.handle_close(&x, reply_to),
            SocketReq::Send(ref x) => self.handle_send(&x, reply_to),
        }
    }

    /// Open a new socket with the given parameters.
    fn handle_bind(&mut self, msg: &ReqBind, reply_to: &super::MessageSender) {
        // let cfm = match net::TcpListener::bind((msg.addr.as_str(), msg.port)) {
        //     Ok(x) => {
        //         let h = self.next_listen;
        //         self.next_listen += 1;
        //         // Spawn a listen thread
        //         let reply_to_copy = reply_to.clone();
        //         let tcp_listener_copy = x.try_clone().unwrap();
        //         let t = thread::spawn(move || listen(tcp_listener_copy, reply_to_copy));
        //         // Make a ListeningSocket object and pop it in the hashmap
        //         let l = ListeningSocket {
        //             handle: h,
        //             socket: x,
        //             thread: t,
        //         };
        //         self.listeners.insert(h, l);
        //         // Send the response
        //         CfmBind { result: Ok(h) }
        //     }
        // };
        let cfm = CfmBind { result: Err(SocketError::Unknown) };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
    }

    /// Handle a ReqUnbind.
    fn handle_unbind(&mut self, msg: &ReqUnbind, reply_to: &super::MessageSender) {
        let cfm = CfmClose {
            result: Err(SocketError::Unknown),
            handle: msg.handle,
        };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
    }

    /// Handle a ReqClose
    fn handle_close(&mut self, msg: &ReqClose, reply_to: &super::MessageSender) {
        let cfm = CfmClose {
            result: Err(SocketError::Unknown),
            handle: msg.handle,
        };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
    }

    /// Handle a ReqSend
    fn handle_send(&mut self, msg: &ReqSend, reply_to: &super::MessageSender) {
        let cfm = CfmSend {
            result: Err(SocketError::Unknown),
            handle: msg.handle,
        };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
    }
}

fn listen(listener: net::TcpListener, reply_to: super::MessageSender) {
    for stream in listener.incoming() {
        match stream {
            // Spawn a connection thread
            Ok(stream) => {
                debug!("Connection! {:?}", stream);
                // Inform the user about the connection
                let ind = IndConnected {
                    open_handle: 1,
                    listen_handle: 2,
                };
                reply_to.send(ind.wrap()).expect("Failed to send!");
                // thread::spawn(move || {
                //    handle_client(stream, reply_to)
                // });
            }
            Err(e) => debug!("Failed connection! {}", e),
        }
    }
}

/// ReqBind is sendable over a channel
impl super::RequestSendable for ReqBind {
    fn wrap(self, reply_to: &super::MessageSender) -> super::Message {
        super::Message::Request(reply_to.clone(),
                                super::Request::Socket(SocketReq::Bind(Box::new(self))))
    }
}

/// ReqUnbind is sendable over a channel
impl super::RequestSendable for ReqUnbind {
    fn wrap(self, reply_to: &super::MessageSender) -> super::Message {
        super::Message::Request(reply_to.clone(),
                                super::Request::Socket(SocketReq::Unbind(Box::new(self))))
    }
}

/// ReqClose is sendable over a channel
impl super::RequestSendable for ReqClose {
    fn wrap(self, reply_to: &super::MessageSender) -> super::Message {
        super::Message::Request(reply_to.clone(),
                                super::Request::Socket(SocketReq::Close(Box::new(self))))
    }
}

/// ReqSend is sendable over a channel
impl super::RequestSendable for ReqSend {
    fn wrap(self, reply_to: &super::MessageSender) -> super::Message {
        super::Message::Request(reply_to.clone(),
                                super::Request::Socket(SocketReq::Send(Box::new(self))))
    }
}

/// CfmBind is sendable over a channel
impl super::NonRequestSendable for CfmBind {
    fn wrap(self) -> super::Message {
        super::Message::Confirmation(super::Confirmation::Socket(SocketCfm::Bind(Box::new(self))))
    }
}

/// CfmUnbind is sendable over a channel
impl super::NonRequestSendable for CfmUnbind {
    fn wrap(self) -> super::Message {
        super::Message::Confirmation(super::Confirmation::Socket(SocketCfm::Unbind(Box::new(self))))
    }
}

/// CfmClose is sendable over a channel
impl super::NonRequestSendable for CfmClose {
    fn wrap(self) -> super::Message {
        super::Message::Confirmation(super::Confirmation::Socket(SocketCfm::Close(Box::new(self))))
    }
}

/// CfmSend is sendable over a channel
impl super::NonRequestSendable for CfmSend {
    fn wrap(self) -> super::Message {
        super::Message::Confirmation(super::Confirmation::Socket(SocketCfm::Send(Box::new(self))))
    }
}

/// IndConnected is sendable over a channel
impl super::NonRequestSendable for IndConnected {
    fn wrap(self) -> super::Message {
        super::Message::Indication(super::Indication::Socket(SocketInd::Connected(Box::new(self))))
    }
}

/// IndDropped is sendable over a channel
impl super::NonRequestSendable for IndDropped {
    fn wrap(self) -> super::Message {
        super::Message::Indication(super::Indication::Socket(SocketInd::Dropped(Box::new(self))))
    }
}

/// IndReceived is sendable over a channel
impl super::NonRequestSendable for IndReceived {
    fn wrap(self) -> super::Message {
        super::Message::Indication(super::Indication::Socket(SocketInd::Received(Box::new(self))))
    }
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
