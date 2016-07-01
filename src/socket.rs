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

use mio::prelude::*;
use mio;
use std::collections::HashMap;
use std::io;
use std::thread;
use std::net;
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
    /// The address to bind to
    pub addr: net::SocketAddr,
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
pub type ListenHandle = usize;

/// Uniquely identifies an open socket
pub type OpenHandle = usize;

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
    reply_to: super::MessageSender,
    listener: mio::tcp::TcpListener,
}

/// Created for every connection receieved on a ListeningSocket
struct ConnectedSocket {
    parent: ListenHandle,
    reply_to: super::MessageSender,
    handle: OpenHandle
    connection: mio::tcp::TcpStream
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
    panic!("This task should never die!");
}

impl mio::Handler for TaskContext {
    type Timeout = u32;
    type Message = super::Message;
    fn ready(&mut self,
             event_loop: &mut mio::EventLoop<TaskContext>,
             token: mio::Token,
             events: mio::EventSet) {
        debug!("In ready with token {:?}", token);
        if let Some(l) = self.listeners.get(&token.0) {
            debug!("And it's a listen token!");
            // @todo remove this unwrap
            let conn = l.listener.accept().unwrap();
            let h = self.next_open;
            self.next_open += 1;
            debug!("Allocated open handle {}", h);
            let l = ConnectedSocket {
                parent: token.0,
                handle: h,
                reply_to: l.reply_to.clone(),
                connection: connection,
            };
            match event_loop.register(&l.listener,
                                      mio::Token(h),
                                      mio::EventSet::readable(),
                                      mio::PollOpt::edge()) {
                Ok(_) => {
                    self.listeners.insert(h, l);
                    CfmBind { result: Ok(h) }
                }
                Err(io_error) => CfmBind { result: Err(SocketError::IOError(io_error)) },
            }
        }
    }
    fn notify(&mut self, event_loop: &mut mio::EventLoop<TaskContext>, msg: super::Message) {
        match msg {
            // We only handle our own requests
            super::Message::Request(reply_to, super::Request::Socket(x)) => {
                self.handle_req(event_loop, &x, &reply_to)
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
    pub fn handle_req(&mut self,
                      event_loop: &mut mio::EventLoop<TaskContext>,
                      msg: &SocketReq,
                      reply_to: &super::MessageSender) {
        debug!("request: {:?}", msg);
        match *msg {
            SocketReq::Bind(ref x) => self.handle_bind(event_loop, &x, reply_to),
            SocketReq::Unbind(ref x) => self.handle_unbind(event_loop, &x, reply_to),
            SocketReq::Close(ref x) => self.handle_close(event_loop, &x, reply_to),
            SocketReq::Send(ref x) => self.handle_send(event_loop, &x, reply_to),
        }
    }

    /// Open a new socket with the given parameters.
    fn handle_bind(&mut self,
                   event_loop: &mut mio::EventLoop<TaskContext>,
                   msg: &ReqBind,
                   reply_to: &super::MessageSender) {
        debug!("In handle_bind");
        let cfm = match mio::tcp::TcpListener::bind(&msg.addr) {
            Ok(server) => {
                let h = self.next_listen;
                self.next_listen += 1;
                debug!("Allocated listen handle {}", h);
                let l = ListeningSocket {
                    handle: h,
                    reply_to: reply_to.clone(),
                    listener: server,
                };
                match event_loop.register(&l.listener,
                                          mio::Token(h),
                                          mio::EventSet::readable(),
                                          mio::PollOpt::edge()) {
                    Ok(_) => {
                        self.listeners.insert(h, l);
                        CfmBind { result: Ok(h) }
                    }
                    Err(io_error) => CfmBind { result: Err(SocketError::IOError(io_error)) },
                }
            }
            Err(io_error) => CfmBind { result: Err(SocketError::IOError(io_error)) },
        };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
    }

    /// Handle a ReqUnbind.
    fn handle_unbind(&mut self,
                     event_loop: &mut mio::EventLoop<TaskContext>,
                     msg: &ReqUnbind,
                     reply_to: &super::MessageSender) {
        let cfm = CfmClose {
            result: Err(SocketError::Unknown),
            handle: msg.handle,
        };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
    }

    /// Handle a ReqClose
    fn handle_close(&mut self,
                    event_loop: &mut mio::EventLoop<TaskContext>,
                    msg: &ReqClose,
                    reply_to: &super::MessageSender) {
        let cfm = CfmClose {
            result: Err(SocketError::Unknown),
            handle: msg.handle,
        };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
    }

    /// Handle a ReqSend
    fn handle_send(&mut self,
                   event_loop: &mut mio::EventLoop<TaskContext>,
                   msg: &ReqSend,
                   reply_to: &super::MessageSender) {
        let cfm = CfmSend {
            result: Err(SocketError::Unknown),
            handle: msg.handle,
        };
        reply_to.send(cfm.wrap()).expect("Couldn't send message");
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
