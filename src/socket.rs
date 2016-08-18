//! # socket - A TCP server task
//!
//! The grease socket task makes handling sockets easier. A user requests the
//! socket task opens a new socket and it does so (if possible). The user then
//! receives asynchronous indications when data arrives on the socket and/or
//! when the socket closes.

#![allow(dead_code)]
#![deny(missing_docs)]

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use std::collections::{HashMap, VecDeque};
use std::convert::From;
use std::fmt;
use std::io::prelude::*;
use std::io;
use std::net;
use std::thread;

use mio::prelude::*;
use mio;

use ::prelude::*;

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
    /// A Bind Request - Bind a listen socket
    Bind(Box<ReqBind>),
    /// An Unbind Request - Unbind a bound listen socket
    Unbind(Box<ReqUnbind>),
    /// A Close request - Close an open connection
    Close(Box<ReqClose>),
    /// A Send request - Send something on a connection
    Send(Box<ReqSend>),
}

/// Confirmations sent from the Socket task in answer to a SocketReq
#[derive(Debug)]
pub enum SocketCfm {
    /// A Bind Confirm - Bound a listen socket
    Bind(Box<CfmBind>),
    /// An Unbind Confirm - Unbound a bound listen socket
    Unbind(Box<CfmUnbind>),
    /// A Close Confirm - Closed an open connection
    Close(Box<CfmClose>),
    /// A Send Confirm - Sent something on a connection
    Send(Box<CfmSend>),
}

/// Asynchronous indications sent by the Socket task
#[derive(Debug)]
pub enum SocketInd {
    /// A Connected Indication - Indicates that a listening socket has been connected to
    Connected(Box<IndConnected>),
    /// A Dropped Indication - Indicates that an open socket has been dropped
    Dropped(Box<IndDropped>),
    /// A Received Indication - Indicates that data has arrived on an open socket
    Received(Box<IndReceived>),
}

/// Responses to Indications required
#[derive(Debug)]
pub enum SocketRsp {
    /// a Receieved Response - unblocks the open socket so more IndReceived can be sent
    Received(Box<RspReceived>),
}

/// Bind a listen socket
#[derive(Debug)]
pub struct ReqBind {
    /// The address to bind to
    pub addr: net::SocketAddr,
    /// Reflected in the cfm
    pub context: ::Context,
}

make_request!(ReqBind, ::Request::Socket, SocketReq::Bind);

/// Unbind a bound listen socket
#[derive(Debug)]
pub struct ReqUnbind {
    /// The handle from a CfmBind
    pub handle: ConnectedHandle,
    /// Reflected in the cfm
    pub context: ::Context,
}

make_request!(ReqUnbind, ::Request::Socket, SocketReq::Unbind);

/// Close an open connection
#[derive(Debug)]
pub struct ReqClose {
    /// The handle from a IndConnected
    pub handle: ConnectedHandle,
    /// Reflected in the cfm
    pub context: ::Context,
}

make_request!(ReqClose, ::Request::Socket, SocketReq::Close);

/// Send something on a connection
pub struct ReqSend {
    /// The handle from a CfmBind
    pub handle: ConnectedHandle,
    /// Some (maybe) unique identifier
    pub context: ::Context,
    /// The data to be sent
    pub data: Vec<u8>,
}

make_request!(ReqSend, ::Request::Socket, SocketReq::Send);

/// Reply to a ReqBind.
#[derive(Debug)]
pub struct CfmBind {
    /// Either a new ListenHandle or an error
    pub result: Result<ListenHandle, SocketError>,
    /// Reflected from the req
    pub context: ::Context,
}

make_confirmation!(CfmBind, ::Confirmation::Socket, SocketCfm::Bind);

/// Reply to a ReqUnbind.
#[derive(Debug)]
pub struct CfmUnbind {
    /// The handle requested for unbinding
    pub handle: ListenHandle,
    /// Whether we were successful in unbinding
    pub result: Result<(), SocketError>,
    /// Reflected from the req
    pub context: ::Context,
}

make_confirmation!(CfmUnbind, ::Confirmation::Socket, SocketCfm::Unbind);

/// Reply to a ReqClose. Will flush out all
/// existing data.
#[derive(Debug)]
pub struct CfmClose {
    /// The handle requested for closing
    pub handle: ConnectedHandle,
    /// Success or failed
    pub result: Result<(), SocketError>,
    /// Reflected from the req
    pub context: ::Context,
}

make_confirmation!(CfmClose, ::Confirmation::Socket, SocketCfm::Close);

/// Reply to a ReqSend. The data has not necessarily
/// been sent, but it is safe to send some more data.
#[derive(Debug)]
pub struct CfmSend {
    /// The handle requested for sending
    pub handle: ConnectedHandle,
    /// Amount sent or error
    pub result: Result<::Context, SocketError>,
    /// Some (maybe) unique identifier
    pub context: ::Context,
}

make_confirmation!(CfmSend, ::Confirmation::Socket, SocketCfm::Send);

/// Indicates that a listening socket has been connected to.
#[derive(Debug)]
pub struct IndConnected {
    /// The listen handle the connection came in on
    pub listen_handle: ListenHandle,
    /// The handle for the new connection
    pub open_handle: ConnectedHandle,
    /// Details about who connected
    pub peer: net::SocketAddr,
}

make_indication!(IndConnected, ::Indication::Socket, SocketInd::Connected);

/// Indicates that a socket has been dropped.
#[derive(Debug)]
pub struct IndDropped {
    /// The handle that is no longer valid
    pub handle: ConnectedHandle,
}

make_indication!(IndDropped, ::Indication::Socket, SocketInd::Dropped);

/// Indicates that data has arrived on the socket
/// No further data will be sent on this handle until
/// RspReceived is sent back. Note that this type
/// has a custom std::fmt::Debug implementation so it
/// doesn't print the (lengthy) contents of `data`.
pub struct IndReceived {
    /// The handle for the socket data came in on
    pub handle: ConnectedHandle,
    /// The data that came in (might be a limited to a small amount)
    pub data: Vec<u8>,
}

make_indication!(IndReceived, ::Indication::Socket, SocketInd::Received);

/// Tell the task that more data can now be sent.
#[derive(Debug)]
pub struct RspReceived {
    /// Which handle is now free to send up more data
    pub handle: ConnectedHandle,
}

make_response!(RspReceived, ::Response::Socket, SocketRsp::Received);

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Users of the socket task should implement this trait to
/// make handling the incoming SocketCfm and SocketInd a little
/// easier.
pub trait User {
    /// Handles a Socket Confirmation, such as you will receive after sending
    /// a Socket Request, by unpacking the enum and routing the struct
    /// contained within to the appropriate handler.
    fn handle_socket_cfm(&mut self, msg: &SocketCfm) {
        match *msg {
            SocketCfm::Bind(ref x) => self.handle_socket_cfm_bind(&x),
            SocketCfm::Unbind(ref x) => self.handle_socket_cfm_unbind(&x),
            SocketCfm::Close(ref x) => self.handle_socket_cfm_close(&x),
            SocketCfm::Send(ref x) => self.handle_socket_cfm_send(&x),
        }
    }

    /// Called when a Bind confirmation is received.
    fn handle_socket_cfm_bind(&mut self, msg: &CfmBind);

    /// Called when an Unbind confirmation is received.
    fn handle_socket_cfm_unbind(&mut self, msg: &CfmUnbind);

    /// Called when a Close confirmation is received.
    fn handle_socket_cfm_close(&mut self, msg: &CfmClose);

    /// Called when a Send confirmation is received.
    fn handle_socket_cfm_send(&mut self, msg: &CfmSend);

    /// Handles a Socket Indication by unpacking the enum and routing the
    /// struct contained withing to the appropriate handler.
    fn handle_socket_ind(&mut self, msg: &SocketInd) {
        match *msg {
            SocketInd::Connected(ref x) => self.handle_socket_ind_connected(&x),
            SocketInd::Dropped(ref x) => self.handle_socket_ind_dropped(&x),
            SocketInd::Received(ref x) => self.handle_socket_ind_received(&x),
        }
    }

    /// Handles a Connected indication.
    fn handle_socket_ind_connected(&mut self, msg: &IndConnected);

    /// Handles a connection Dropped indication.
    fn handle_socket_ind_dropped(&mut self, msg: &IndDropped);

    /// Handles a data Received indication.
    fn handle_socket_ind_received(&mut self, msg: &IndReceived);
}

/// Uniquely identifies an listening socket
pub type ListenHandle = ::Context;

/// Uniquely identifies an open socket
pub type ConnectedHandle = ::Context;

/// All possible errors the Socket task might want to
/// report.
#[derive(Debug, Copy, Clone)]
pub enum SocketError {
    /// An underlying socket error
    IOError(io::ErrorKind),
    /// The given handle was not recognised
    BadHandle,
    /// The pending write failed because the socket dropped
    Dropped,
    /// Function not implemented yet
    NotImplemented,
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
    ind_to: ::MessageSender,
    listener: mio::tcp::TcpListener,
}

/// Create for every pending write
struct PendingWrite {
    context: ::Context,
    sent: usize,
    data: Vec<u8>,
    reply_to: ::MessageSender,
}

/// Created for every connection receieved on a ListenSocket
struct ConnectedSocket {
    parent: ListenHandle,
    ind_to: ::MessageSender,
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

const MAX_READ_LEN: usize = 1024;

// ****************************************************************************
//
// Public Functions
//
// ****************************************************************************

/// Creates a new socket task. Returns an object that can be used
/// to send this task messages.
pub fn make_task() -> ::MessageSender {
    ::make_task("socket", main_loop)
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

/// The task runs this main loop indefinitely.
/// Unfortunately, to use mio, we have to use their special
/// channels. So, we spin up a thread to bounce from one
/// channel to the other. We don't need our own
/// MessageSender as we don't send Requests that need replying to.
fn main_loop(rx: ::MessageReceiver, _: ::MessageSender) {
    let mut event_loop = mio::EventLoop::new().unwrap();
    let ch = event_loop.channel();
    let _ = thread::spawn(move || {
        for msg in rx.iter() {
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
    type Message = ::Message;

    /// Called when mio has an update on a registered listener or connection
    /// We have to check the EventSet to find out whether our socket is
    /// readable or writable
    fn ready(&mut self, event_loop: &mut TaskEventLoop, token: mio::Token, events: mio::EventSet) {
        debug!("Ready!");
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
        debug!("Ready is done");
    }

    /// Called when mio (i.e. our task) has received a message
    fn notify(&mut self, event_loop: &mut TaskEventLoop, msg: ::Message) {
        debug!("Notify!");
        match msg {
            // We only handle our own requests and responses
            ::Message::Request(ref reply_to, ::Request::Socket(ref x)) => {
                self.handle_socket_req(event_loop, x, reply_to)
            }
            ::Message::Request(ref reply_to, ::Request::Generic(ref x)) => {
                self.handle_generic_req(x, reply_to)
            }
            ::Message::Response(::Response::Socket(ref x)) => self.handle_socket_rsp(event_loop, x),
            // We don't expect any Indications or Confirmations from other providers
            // If we get here, someone else has made a mistake
            _ => error!("Unexpected message in socket task: {:?}", msg),
        }
        debug!("Notify is done");
    }

    /// Should never be called - we don't have a timeout on our poll
    fn timeout(&mut self, _: &mut TaskEventLoop, _: Self::Timeout) {}

    /// Not sure when this is called
    fn interrupted(&mut self, _: &mut TaskEventLoop) {}

    /// Not sure when this is called but I don't think we need to
    /// do anything
    fn tick(&mut self, _: &mut TaskEventLoop) {}
}

impl TaskContext {
    /// Init the context
    pub fn new() -> TaskContext {
        TaskContext {
            listeners: HashMap::new(),
            connections: HashMap::new(),
            // It helps the debug to keep these two apart
            next_listen: 3_000,
            next_open: 4_000,
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
                ind_to: ls.ind_to.clone(),
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
                    let ind = IndConnected {
                        listen_handle: ls.handle,
                        open_handle: cs.handle,
                        peer: conn_addr.1,
                    };
                    self.connections.insert(cs.handle, cs);
                    ls.ind_to.send_nonrequest(ind);
                }
                Err(err) => warn!("Fumbled incoming connection: {}", err),
            }
        } else {
            warn!("accept returned None!");
        }
    }

    /// Data can be sent a connected socket. Send what we have
    fn pending_writes(&mut self, _: &mut TaskEventLoop, cs_handle: ConnectedHandle) {
        // We know this exists because we checked it before we got here
        let mut cs = self.connections.get_mut(&cs_handle).unwrap();
        loop {
            if let Some(mut pw) = cs.pending_writes.pop_front() {
                let to_send = pw.data.len() - pw.sent;
                match cs.connection.write(&pw.data[pw.sent..]) {
                    Ok(len) if len < to_send => {
                        let left = to_send - len;
                        debug!("Sent {} of {}, leaving {}", len, to_send, left);
                        pw.sent = pw.sent + len;
                        cs.pending_writes.push_front(pw);
                        // No indication here - we wait some more
                        break;
                    }
                    Ok(_) => {
                        debug!("Sent all {}", to_send);
                        let cfm = CfmSend {
                            handle: cs.handle,
                            context: pw.context,
                            result: Ok(to_send),
                        };
                        pw.reply_to.send_nonrequest(cfm);
                    }
                    Err(err) => {
                        warn!("Send error: {}", err);
                        let cfm = CfmSend {
                            handle: cs.handle,
                            context: pw.context,
                            result: Err(err.into()),
                        };
                        pw.reply_to.send_nonrequest(cfm);
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    /// Data is available on a connected socket. Pass it up
    fn read(&mut self, _: &mut TaskEventLoop, cs_handle: ConnectedHandle) {
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
                    cs.ind_to.send_nonrequest(ind);
                }
                Err(_) => {}
            }
        }
    }

    /// Connection has gone away. Clean up.
    fn dropped(&mut self, _: &mut TaskEventLoop, cs_handle: ConnectedHandle) {
        // We know this exists because we checked it before we got here
        {
            let cs = self.connections.get(&cs_handle).unwrap();
            let ind = IndDropped { handle: cs_handle };
            cs.ind_to.send_nonrequest(ind);
        }
        self.connections.remove(&cs_handle);
    }

    /// Handle requests
    pub fn handle_socket_req(&mut self,
                             event_loop: &mut TaskEventLoop,
                             req: &SocketReq,
                             reply_to: &::MessageSender) {
        debug!("request: {:?}", req);
        match *req {
            SocketReq::Bind(ref x) => self.handle_bind(event_loop, x, reply_to),
            SocketReq::Unbind(ref x) => self.handle_unbind(event_loop, x, reply_to),
            SocketReq::Close(ref x) => self.handle_close(event_loop, x, reply_to),
            SocketReq::Send(ref x) => self.handle_send(event_loop, x, reply_to),
        }
    }

    /// Open a new socket with the given parameters.
    fn handle_bind(&mut self,
                   event_loop: &mut TaskEventLoop,
                   req_bind: &ReqBind,
                   reply_to: &::MessageSender) {
        info!("Binding {}...", req_bind.addr);
        let cfm = match mio::tcp::TcpListener::bind(&req_bind.addr) {
            Ok(server) => {
                let h = self.next_listen;
                self.next_listen += 1;
                debug!("Allocated listen handle {}", h);
                let l = ListenSocket {
                    handle: h,
                    // We assume any future indications should be sent
                    // to the same place we send the CfmBind.
                    ind_to: reply_to.clone(),
                    listener: server,
                };
                match event_loop.register(&l.listener,
                                          mio::Token(h),
                                          mio::EventSet::readable(),
                                          mio::PollOpt::edge()) {
                    Ok(_) => {
                        self.listeners.insert(h, l);
                        CfmBind {
                            result: Ok(h),
                            context: req_bind.context,
                        }
                    }
                    Err(io_error) => {
                        CfmBind {
                            result: Err(io_error.into()),
                            context: req_bind.context,
                        }
                    }
                }
            }
            Err(io_error) => {
                CfmBind {
                    result: Err(io_error.into()),
                    context: req_bind.context,
                }
            }
        };
        reply_to.send_nonrequest(cfm);
    }

    /// Handle a ReqUnbind.
    fn handle_unbind(&mut self,
                     _: &mut TaskEventLoop,
                     req_unbind: &ReqUnbind,
                     reply_to: &::MessageSender) {
        let cfm = CfmClose {
            result: Err(SocketError::NotImplemented),
            handle: req_unbind.handle,
            context: req_unbind.context,
        };
        reply_to.send_nonrequest(cfm);
    }

    /// Handle a ReqClose
    fn handle_close(&mut self,
                    _: &mut TaskEventLoop,
                    req_close: &ReqClose,
                    reply_to: &::MessageSender) {
        let cfm = CfmClose {
            result: Err(SocketError::NotImplemented),
            handle: req_close.handle,
            context: req_close.context,
        };
        reply_to.send_nonrequest(cfm);
    }

    /// Handle a ReqSend
    fn handle_send(&mut self,
                   _: &mut TaskEventLoop,
                   req_send: &ReqSend,
                   reply_to: &::MessageSender) {
        if let Some(cs) = self.connections.get_mut(&req_send.handle) {
            let to_send = req_send.data.len();
            // Let's see how much we can get rid off right now
            if cs.pending_writes.len() > 0 {
                debug!("Storing write len {}", to_send);
                let pw = PendingWrite {
                    sent: 0,
                    context: req_send.context,
                    data: req_send.data.clone(),
                    reply_to: reply_to.clone(),
                };
                cs.pending_writes.push_back(pw);
                // No cfm here - we wait
            } else {
                match cs.connection.write(&req_send.data) {
                    Ok(len) if len < to_send => {
                        let left = to_send - len;
                        debug!("Sent {} of {}, leaving {}", len, to_send, left);
                        let pw = PendingWrite {
                            sent: len,
                            context: req_send.context,
                            data: req_send.data.clone(),
                            reply_to: reply_to.clone(),
                        };
                        cs.pending_writes.push_back(pw);
                        // No cfm here - we wait
                    }
                    Ok(_) => {
                        debug!("Sent all {}", to_send);
                        let cfm = CfmSend {
                            context: req_send.context,
                            handle: req_send.handle,
                            result: Ok(to_send),
                        };
                        reply_to.send_nonrequest(cfm);
                    }
                    Err(err) => {
                        warn!("Send error: {}", err);
                        let cfm = CfmSend {
                            context: req_send.context,
                            handle: req_send.handle,
                            result: Err(err.into()),
                        };
                        reply_to.send_nonrequest(cfm);
                    }
                }
            }
        } else {
            let cfm = CfmSend {
                result: Err(SocketError::BadHandle),
                context: req_send.context,
                handle: req_send.handle,
            };
            reply_to.send_nonrequest(cfm);
        }
    }

    /// Handle responses
    pub fn handle_socket_rsp(&mut self, event_loop: &mut TaskEventLoop, rsp: &SocketRsp) {
        match *rsp {
            SocketRsp::Received(ref x) => self.handle_received(event_loop, x),
        }
    }

    /// Someone wants more data
    fn handle_received(&mut self, event_loop: &mut TaskEventLoop, rsp_received: &RspReceived) {
        let mut need_read = false;
        // Read response handle might not be valid - it might
        // have crossed over with a disconnect.
        if let Some(cs) = self.connections.get_mut(&rsp_received.handle) {
            cs.outstanding = false;
            // Let's try and send them some - if if exhausts the
            // buffer on the socket, the event loop will automatically
            // set itself to interrupt when more data arrives
            need_read = true;
        }
        if need_read {
            // Try and read it - won't hurt if we can't.
            self.read(event_loop, rsp_received.handle)
        }
    }
}

impl GenericProvider for TaskContext {}

impl Drop for ConnectedSocket {
    fn drop(&mut self) {
        for pw in self.pending_writes.iter() {
            let cfm = CfmSend {
                handle: self.handle,
                context: pw.context,
                result: Err(SocketError::Dropped),
            };
            pw.reply_to.send_nonrequest(cfm);
        }
    }
}

#[cfg(test)]
mod test {
    use std::thread;
    use std::time;

    #[test]
    fn make_task_and_ping() {
        let socket_thread = super::make_task();
        let (reply_to, test_rx) = ::make_channel();
        let ping_req = ::PingReq { context: 1234 };
        thread::sleep(time::Duration::new(5, 0));
        socket_thread.send_request(ping_req, &reply_to);
        let cfm = test_rx.recv().unwrap();
        match cfm {
            ::Message::Confirmation(::Confirmation::Generic(::GenericCfm::Ping(ref x))) => {
                assert_eq!(x.context, 1234);
            }
            _ => panic!("Bad match"),
        }
    }
}

/// Don't log the contents of the vector
impl fmt::Debug for IndReceived {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
               "IndReceived {{ handle: {}, data.len: {} }}",
               self.handle,
               self.data.len())
    }
}

/// Don't log the contents of the vector
impl fmt::Debug for ReqSend {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
               "ReqSend {{ handle: {}, data.len: {} }}",
               self.handle,
               self.data.len())
    }
}

/// Wrap io::Errors into SocketErrors easily
impl From<io::Error> for SocketError {
    fn from(e: io::Error) -> SocketError {
        SocketError::IOError(e.kind())
    }
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
