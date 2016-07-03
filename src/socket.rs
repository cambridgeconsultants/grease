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
use std::collections::{HashMap, VecDeque};
use std::io::prelude::*;
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
    Connected(Box<IndConnected>),
    Dropped(Box<IndDropped>),
    Received(Box<IndReceived>),
}

/// Responses to Indications required
#[derive(Debug)]
pub enum SocketRsp {
    Received(Box<RspReceived>),
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
    pub handle: ConnectedHandle,
}

/// Close an open connection
#[derive(Debug)]
pub struct ReqClose {
    /// The handle from a IndConnected
    pub handle: ConnectedHandle,
}

/// Send something on a connection
#[derive(Debug)]
pub struct ReqSend {
    /// The handle from a CfmBind
    pub handle: ConnectedHandle,
    /// Some (maybe) unique identifier
    pub context: WriteContext,
    /// The data to be sent
    pub data: Vec<u8>,
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
    pub handle: ConnectedHandle,
    pub result: Result<(), SocketError>,
}

/// Reply to a ReqSend. The data has not necessarily
/// been sent, but it is safe to send some more data.
#[derive(Debug)]
pub struct CfmSend {
    pub handle: ConnectedHandle,
    pub result: Result<(), SocketError>,
    /// Some (maybe) unique identifier
    pub context: WriteContext,
}

/// Indicates that a listening socket has been connected to.
#[derive(Debug)]
pub struct IndConnected {
    pub listen_handle: ListenHandle,
    pub open_handle: ConnectedHandle,
    pub peer: net::SocketAddr,
}

/// Indicates that a socket has been dropped.
#[derive(Debug)]
pub struct IndDropped {
    pub handle: ConnectedHandle,
}

/// Indicates that data has arrived on the socket
/// No further data will be sent on this handle until
/// RspReceived is sent back.
#[derive(Debug)]
pub struct IndReceived {
    pub handle: ConnectedHandle,
    pub data: Vec<u8>,
}

/// Tell the task that more data can now be sent.
#[derive(Debug)]
pub struct RspReceived {
    pub handle: ConnectedHandle,
}

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Uniquely identifies an listening socket
pub type ListenHandle = usize;

/// Uniquely identifies an open socket
pub type ConnectedHandle = usize;

/// Allows a user to track multiple writes
pub type WriteContext = usize;

/// All possible errors the Socket task might want to
/// report.
#[derive(Debug)]
pub enum SocketError {
    IOError(io::Error),
    BadHandle,
    Dropped,
    BadAddress,
    Timeout,
    Unknown,
}

// ****************************************************************************
//
// Private Types
//
// ****************************************************************************

type TaskEventLoop = EventLoop<TaskContext>;

/// Created for every bound (i.e. listening) socket
struct ListenSocket {
    handle: ListenHandle,
    reply_to: super::MessageSender,
    listener: mio::tcp::TcpListener,
}

/// Create for every pending write
struct PendingWrite {
    context: WriteContext,
    data: Vec<u8>,
}

/// Created for every connection receieved on a ListenSocket
struct ConnectedSocket {
    parent: ListenHandle,
    reply_to: super::MessageSender,
    handle: ConnectedHandle,
    connection: mio::tcp::TcpStream,
    /// There's a read the user hasn't process yet
    outstanding: bool,
    /// Queue of pending writes
    pending_writes: VecDeque<PendingWrite>,
}

/// One instance per task. Stores all the task data.
struct TaskContext {
    /// Set of all bound sockets
    listeners: HashMap<ListenHandle, ListenSocket>,
    /// Set of all connected sockets
    connections: HashMap<ConnectedHandle, ConnectedSocket>,
    /// The next handle we'll use for a bound socket
    next_listen: ListenHandle,
    /// The next handle we'll use for an open socket
    next_open: ConnectedHandle,
}

// ****************************************************************************
//
// Public Data
//
// ****************************************************************************

// None

// ****************************************************************************
//
// Private Data
//
// ****************************************************************************

const MAX_READ_LEN: usize = 64;

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

/// These are our mio callbacks. They are called by the mio EventLoop
/// when interesting things happen. We deal with the interesting thing, then
/// the EventLoop calls another callback or waits for more interesting things.
impl mio::Handler for TaskContext {
    type Timeout = u32;
    type Message = super::Message;

    /// Called when mio has an update on a registered listener or connection
    /// We have to check the EventSet to find out whether our socket is
    /// readable or writable
    fn ready(&mut self, event_loop: &mut TaskEventLoop, token: mio::Token, events: mio::EventSet) {
        let handle = token.0;
        if events.is_readable() {
            if self.listeners.contains_key(&handle) {
                self.accept(event_loop, handle)
            } else if self.connections.contains_key(&handle) {
                self.read(event_loop, handle)
            } else {
                warn!("Readable on unknown token {}", handle);
            }
        }
        if events.is_writable() {
            if self.listeners.contains_key(&handle) {
                debug!("Writable listen socket {}?", handle);
            } else if self.connections.contains_key(&handle) {
                debug!("Writable connected socket {}", handle);
                self.pending_writes(event_loop, handle);
            } else {
                warn!("Writable on unknown token {}", handle);
            }
        }
        if events.is_error() {
            if self.listeners.contains_key(&handle) {
                debug!("Error listen socket {}", handle);
            } else if self.connections.contains_key(&handle) {
                debug!("Error connected socket {}", handle);
            } else {
                warn!("Error on unknown token {}", handle);
            }
        }
        if events.is_hup() {
            if self.listeners.contains_key(&handle) {
                debug!("HUP listen socket {}", handle);
            } else if self.connections.contains_key(&handle) {
                debug!("HUP connected socket {}", handle);
                self.dropped(event_loop, handle);
            } else {
                warn!("HUP on unknown token {}", handle);
            }
        }
    }

    /// Called when mio (i.e. our task) has received a message
    fn notify(&mut self, event_loop: &mut TaskEventLoop, msg: super::Message) {
        match msg {
            // We only handle our own requests and responses
            super::Message::Request(reply_to, super::Request::Socket(x)) => {
                self.handle_req(event_loop, &x, &reply_to)
            }
            super::Message::Response(super::Response::Socket(x)) => self.handle_rsp(event_loop, &x),
            // We don't have any responses
            // We don't expect any Indications or Confirmations from other providers
            // If we get here, someone else has made a mistake
            _ => error!("Unexpected message in socket task: {:?}", msg),
        }
    }

    /// Should never be called - we don't have a timeout on our poll
    fn timeout(&mut self, event_loop: &mut TaskEventLoop, timeout: Self::Timeout) {
        unimplemented!();
    }

    /// Not sure when this is called
    fn interrupted(&mut self, event_loop: &mut TaskEventLoop) {
        warn!("Interrupted!");
    }

    /// Not sure when this is called but I don't think we need to
    /// do anything
    fn tick(&mut self, event_loop: &mut TaskEventLoop) {
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

    /// Accept a new incoming connection and let the user know
    /// with a IndConnected
    fn accept(&mut self, event_loop: &mut TaskEventLoop, ls_handle: ListenHandle) {
        // We know this exists because we checked it before we got here
        let ls = self.listeners.get(&ls_handle).unwrap();
        if let Some(conn_addr) = ls.listener.accept().unwrap() {
            let cs = ConnectedSocket {
                parent: ls.handle,
                handle: self.next_open,
                reply_to: ls.reply_to.clone(),
                connection: conn_addr.0,
                outstanding: false,
                pending_writes: VecDeque::new(),
            };
            self.next_open += 1;
            match event_loop.register(&cs.connection,
                                      mio::Token(cs.handle),
                                      mio::EventSet::readable() | mio::EventSet::hup() |
                                      mio::EventSet::error() |
                                      mio::EventSet::writable(),
                                      mio::PollOpt::edge()) {
                Ok(_) => {
                    let msg = IndConnected {
                        listen_handle: ls.handle,
                        open_handle: cs.handle,
                        peer: conn_addr.1,
                    };
                    self.connections.insert(cs.handle, cs);
                    ls.reply_to.send(msg.wrap());
                }
                Err(err) => debug!("Fumbled incoming connection: {}", err),
            }
        } else {
            warn!("accept returned None!");
        }
    }

    /// Data can be sent a connected socket. Send what we have
    fn pending_writes(&mut self, event_loop: &mut TaskEventLoop, cs_handle: ConnectedHandle) {
        // We know this exists because we checked it before we got here
        let mut cs = self.connections.get_mut(&cs_handle).unwrap();
        if let Some(mut pw) = cs.pending_writes.pop_front() {
            let to_send = pw.data.len();
            match cs.connection.write(&pw.data) {
                Ok(len) if len < to_send => {
                    debug!("Sent {} of {}", len, to_send);
                    let pw = PendingWrite {
                        context: pw.context,
                        data: pw.data.split_off(len),
                    };
                    cs.pending_writes.push_front(pw);
                    // No indication here - we wait some more
                }
                Ok(_) => {
                    debug!("Sent it all");
                    let cfm = CfmSend {
                        handle: cs.handle,
                        context: pw.context,
                        result: Ok(()),
                    };
                    cs.reply_to.send(cfm.wrap()).expect("Unable to send");
                }
                Err(err) => {
                    warn!("Send error: {}", err);
                    let cfm = CfmSend {
                        handle: cs.handle,
                        context: pw.context,
                        result: Err(SocketError::IOError(err)),
                    };
                    cs.reply_to.send(cfm.wrap()).expect("Unable to send");
                }
            }
        }
    }

    /// Data is available on a connected socket. Pass it up
    fn read(&mut self, event_loop: &mut TaskEventLoop, cs_handle: ConnectedHandle) {
        debug!("Reading connection {}", cs_handle);
        // We know this exists because we checked it before we got here
        let mut cs = self.connections.get_mut(&cs_handle).unwrap();
        // Only pass up one indication at a time
        if !cs.outstanding {
            // Cap the max amount we will read
            let mut buffer = vec![0u8; MAX_READ_LEN];
            match cs.connection.read(buffer.as_mut_slice()) {
                Ok(0) => {}
                Ok(len) => {
                    debug!("Read {} octets", len);
                    let _ = buffer.split_off(len);
                    let ind = IndReceived {
                        handle: cs.handle,
                        data: buffer,
                    };
                    cs.outstanding = true;
                    cs.reply_to.send(ind.wrap()).expect("Failed to send");
                }
                Err(err) => debug!("Failed to read: {}", err),
            }
        }
    }

    /// Connection has gone away. Clean up.
    fn dropped(&mut self, event_loop: &mut TaskEventLoop, cs_handle: ConnectedHandle) {
        // We know this exists because we checked it before we got here
        {
            let cs = self.connections.get(&cs_handle).unwrap();
            let msg = IndDropped { handle: cs_handle };
            cs.reply_to.send(msg.wrap()).expect("Unable to send message");
        }
        self.connections.remove(&cs_handle);
    }

    /// Handle requests
    pub fn handle_req(&mut self,
                      event_loop: &mut TaskEventLoop,
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
                   event_loop: &mut TaskEventLoop,
                   msg: &ReqBind,
                   reply_to: &super::MessageSender) {
        debug!("In handle_bind");
        let cfm = match mio::tcp::TcpListener::bind(&msg.addr) {
            Ok(server) => {
                let h = self.next_listen;
                self.next_listen += 1;
                debug!("Allocated listen handle {}", h);
                let l = ListenSocket {
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
                     event_loop: &mut TaskEventLoop,
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
                    event_loop: &mut TaskEventLoop,
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
                   event_loop: &mut TaskEventLoop,
                   msg: &ReqSend,
                   reply_to: &super::MessageSender) {
        if let Some(cs) = self.connections.get_mut(&msg.handle) {
            let to_send = msg.data.len();
            // Let's see how much we can get rid off right now
            if cs.pending_writes.len() > 0 {
                debug!("Storing write");
                let pw = PendingWrite {
                    context: msg.context,
                    data: msg.data.clone(),
                };
                cs.pending_writes.push_back(pw);
                // No cfm here - we wait
            } else {
                match cs.connection.write(&msg.data) {
                    Ok(len) if len < to_send => {
                        debug!("Sent {} of {}", len, to_send);
                        let pw = PendingWrite {
                            context: msg.context,
                            data: msg.data.clone().split_off(len),
                        };
                        cs.pending_writes.push_back(pw);
                        // No cfm here - we wait
                    }
                    Ok(_) => {
                        debug!("Sent it all");
                        let cfm = CfmSend {
                            context: msg.context,
                            handle: msg.handle,
                            result: Ok(()),
                        };
                        cs.reply_to.send(cfm.wrap()).expect("Unable to send");
                    }
                    Err(err) => {
                        warn!("Send error: {}", err);
                        let cfm = CfmSend {
                            context: msg.context,
                            handle: msg.handle,
                            result: Err(SocketError::IOError(err)),
                        };
                        cs.reply_to.send(cfm.wrap()).expect("Unable to send");
                    }
                }
            }
        } else {
            let cfm = CfmSend {
                result: Err(SocketError::BadHandle),
                context: msg.context,
                handle: msg.handle,
            };
            reply_to.send(cfm.wrap()).expect("Couldn't send message");
        }
    }

    /// Handle responses
    pub fn handle_rsp(&mut self, event_loop: &mut TaskEventLoop, msg: &SocketRsp) {
        debug!("request: {:?}", msg);
        match *msg {
            SocketRsp::Received(ref x) => self.handle_received(event_loop, x),
        }
    }

    /// Someone wants more data
    fn handle_received(&mut self, event_loop: &mut TaskEventLoop, msg: &RspReceived) {
        let mut need_read = false;
        // Read response handle might not be valid - it might
        // have crossed over with a disconnect.
        if let Some(cs) = self.connections.get_mut(&msg.handle) {
            cs.outstanding = false;
            // Let's try and send them some - if if exhausts the
            // buffer on the socket, the event loop will automatically
            // set itself to interrupt when more data arrives
            need_read = true;
        }
        if need_read {
            // Try and read it - won't hurt if we can't.
            self.read(event_loop, msg.handle)
        }
    }
}

impl Drop for ConnectedSocket {
    fn drop(&mut self) {
        for pw in self.pending_writes.iter() {
            let cfm = CfmSend {
                handle: self.handle,
                context: pw.context,
                result: Err(SocketError::Dropped),
            };
            self.reply_to.send(cfm.wrap()).expect("Unable to send");
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

/// RspReceived is sendable over a channel
impl super::NonRequestSendable for RspReceived {
    fn wrap(self) -> super::Message {
        super::Message::Response(super::Response::Socket(SocketRsp::Received(Box::new(self))))
    }
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
