//! # http - The HTTP service
//!
//! This service depends on socket for the socket-y stuff, and
//! it uses the rushttp library to handle the HTTP parsing. The layer
//! above is responsible for working out what to do with the requests.
//!

//! Once an http server has been bound using `ReqBind`, when an http request
//! has been seen, an `IndRequest` will be sent up containing the relevant headers.
//! An `IndClosed` will be sent when the connection subsequently closes. The
//! user of the service should follow an IndRequest with a ReqResponseStart,
//! zero or more ReqResponseBody and then a ReqResponseClose. For flow control
//! it is recommended that the user waits for the CfmResponseBody before sending
//! another ReqResponseBody. Although the socket task underneath should buffer
//! all the data anyway, using flow control properly saves memory - especially
//! when sending large bodies.

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use std::net;
use std::collections::HashMap;

use rushttp;
use ::prelude::*;

// ****************************************************************************
//
// Public Messages
//
// ****************************************************************************

/// Requests that can be sent to the http task.
#[derive(Debug)]
pub enum HttpReq {
    /// A bind request - start an HTTP server on a given port
    Bind(Box<ReqBind>),
    /// Send the headers for an HTTP response
    ResponseStart(Box<ReqResponseStart>),
    /// Send some body content for an HTTP response
    ResponseBody(Box<ReqResponseBody>),
    /// Close out an HTTP response
    ResponseClose(Box<ReqResponseClose>),
}

/// Confirmations that must be sent back to the http task.
#[derive(Debug)]
pub enum HttpCfm {
    /// Whether the ReqBind was successfull
    Bind(Box<CfmBind>),
    /// Whether the ReqResponseStart was successfull
    ResponseStart(Box<CfmResponseStart>),
    /// Confirms a ReqResponseBody has been sent
    ResponseBody(Box<CfmResponseBody>),
    /// Confirms the connection has been closed
    ResponseClose(Box<CfmResponseClose>),
}

/// Indications that come out of the http task.
#[derive(Debug)]
pub enum HttpInd {
    /// A new HTTP request has been received
    Connected(Box<IndConnected>),
    /// An HTTP connection has been dropped
    Closed(Box<IndClosed>),
}

/// A bind request - start an HTTP server on a given port
#[derive(Debug)]
pub struct ReqBind {
    pub addr: net::SocketAddr,
    pub context: ::Context,
}

/// Send the headers for an HTTP response
#[derive(Debug)]
pub struct ReqResponseStart {
    pub handle: ConnectionHandle,
    pub context: ::Context,
}

/// Send some body content for an HTTP response
#[derive(Debug)]
pub struct ReqResponseBody {
    pub handle: ConnectionHandle,
    pub context: ::Context,
}

/// Close out an HTTP response
#[derive(Debug)]
pub struct ReqResponseClose {
    pub handle: ConnectionHandle,
    pub context: ::Context,
}

/// Whether the ReqBind was successfull
#[derive(Debug)]
pub struct CfmBind {
    pub context: ::Context,
    pub result: Result<ServerHandle, HttpErr>,
}

/// Whether the ReqResponseStart was successfull
#[derive(Debug)]
pub struct CfmResponseStart {
    pub handle: ConnectionHandle,
    pub context: ::Context,
    pub result: Result<(), HttpErr>,
}

/// Confirms a ReqResponseBody has been sent
#[derive(Debug)]
pub struct CfmResponseBody {
    pub handle: ConnectionHandle,
    pub context: ::Context,
    pub result: Result<(), HttpErr>,
}

/// Confirms the connection has been closed
#[derive(Debug)]
pub struct CfmResponseClose {
    pub handle: ConnectionHandle,
    pub context: ::Context,
    pub result: Result<(), HttpErr>,
}

/// A new HTTP request has been received
#[derive(Debug)]
pub struct IndConnected {
    pub server_handle: ServerHandle,
    pub connection_handle: ConnectionHandle,
    pub url: String,
    pub method: rushttp::http::HttpMethod,
    pub headers: HashMap<String, String>,
}

/// An HTTP connection has been dropped
#[derive(Debug)]
pub struct IndClosed {
    pub handle: ConnectionHandle,
}

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// A new one of these is allocated for every new HTTP server
pub type ServerHandle = ::Context;

/// A new one of these is allocated for every new HTTP request
pub type ConnectionHandle = ::Context;

/// All possible http task errors
#[derive(Debug)]
pub enum HttpErr {
    /// Used when I'm writing code and haven't added the correct error yet
    Unknown,
    /// Used if a ReqResponseXXX is sent on an invalid (perhaps recently
    /// closed) ConnectionHandle
    BadHandle,
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

struct TaskContext {
    socket: ::MessageSender,
}

// ****************************************************************************
//
// Public Functions
//
// ****************************************************************************

/// Creates a new socket task. Returns an object that can be used
/// to send this task messages.
pub fn make_task(socket: &::MessageSender) -> ::MessageSender {
    let local_socket = socket.clone();
    ::make_task("socket",
                move |rx: ::MessageReceiver| main_loop(rx, local_socket))
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
fn main_loop(rx: ::MessageReceiver, socket: ::MessageSender) {
    let mut t = TaskContext::new(socket);
    loop {
        if let Ok(msg) = rx.recv() {
            t.handle(msg);
        } else {
            break;
        }
    }
    panic!("This task should never die!");
}

impl TaskContext {
    fn new(socket: ::MessageSender) -> TaskContext {
        TaskContext { socket: socket }
    }

    fn handle(&mut self, msg: ::Message) {
        match msg {
            // We only handle our own requests and responses
            ::Message::Request(ref reply_to, ::Request::Http(ref x)) => {
                self.handle_http_req(x, reply_to)
            }
            ::Message::Request(ref reply_to, ::Request::Generic(ref x)) => {
                self.handle_generic_req(x, reply_to)
            }
            // We don't have any responses
            // We don't expect any Indications or Confirmations from other providers
            // If we get here, someone else has made a mistake
            _ => error!("Unexpected message in socket task: {:?}", msg),
        }
    }

    fn handle_http_req(&mut self, msg: &HttpReq, reply_to: &::MessageSender) {
        debug!("request: {:?}", msg);
        match *msg {
            HttpReq::Bind(ref x) => self.handle_bind(x, reply_to),
            HttpReq::ResponseStart(ref x) => self.handle_responsestart(x, reply_to),
            HttpReq::ResponseBody(ref x) => self.handle_responsebody(x, reply_to),
            HttpReq::ResponseClose(ref x) => self.handle_responseclose(x, reply_to),
        }

    }

    fn handle_bind(&mut self, msg: &ReqBind, reply_to: &::MessageSender) {
        debug!("handle_bind: {:?}", msg);
        let cfm = CfmBind {
            context: msg.context,
            result: Err(HttpErr::Unknown),
        };
        reply_to.send(cfm.wrap()).unwrap();
    }

    fn handle_responsestart(&mut self, msg: &ReqResponseStart, reply_to: &::MessageSender) {
        debug!("handle_responsestart: {:?}", msg);
        let cfm = CfmResponseStart {
            context: msg.context,
            handle: msg.handle,
            result: Err(HttpErr::Unknown),
        };
        reply_to.send(cfm.wrap()).unwrap();
    }

    fn handle_responsebody(&mut self, msg: &ReqResponseBody, reply_to: &::MessageSender) {
        debug!("handle_responsebody: {:?}", msg);
        let cfm = CfmResponseBody {
            context: msg.context,
            handle: msg.handle,
            result: Err(HttpErr::Unknown),
        };
        reply_to.send(cfm.wrap()).unwrap();
    }

    fn handle_responseclose(&mut self, msg: &ReqResponseClose, reply_to: &::MessageSender) {
        debug!("handle_responseclose: {:?}", msg);
        let cfm = CfmResponseClose {
            context: msg.context,
            handle: msg.handle,
            result: Err(HttpErr::Unknown),
        };
        reply_to.send(cfm.wrap()).unwrap();
    }

    fn handle_generic_req(&mut self, msg: &::GenericReq, reply_to: &::MessageSender) {
        match *msg {
            ::GenericReq::Ping(ref x) => {
                let cfm = ::PingCfm { context: x.context };
                reply_to.send(cfm.wrap()).unwrap();
            }
        }
    }
}

/// ReqBind is sendable over a channel
impl RequestSendable for ReqBind {
    fn wrap(self, reply_to: &::MessageSender) -> ::Message {
        ::Message::Request(reply_to.clone(),
                           ::Request::Http(HttpReq::Bind(Box::new(self))))
    }
}

/// ReqResponseStart is sendable over a channel
impl RequestSendable for ReqResponseStart {
    fn wrap(self, reply_to: &::MessageSender) -> ::Message {
        ::Message::Request(reply_to.clone(),
                           ::Request::Http(HttpReq::ResponseStart(Box::new(self))))
    }
}

/// ReqResponseBody is sendable over a channel
impl RequestSendable for ReqResponseBody {
    fn wrap(self, reply_to: &::MessageSender) -> ::Message {
        ::Message::Request(reply_to.clone(),
                           ::Request::Http(HttpReq::ResponseBody(Box::new(self))))
    }
}

/// ReqResponseClose is sendable over a channel
impl RequestSendable for ReqResponseClose {
    fn wrap(self, reply_to: &::MessageSender) -> ::Message {
        ::Message::Request(reply_to.clone(),
                           ::Request::Http(HttpReq::ResponseClose(Box::new(self))))
    }
}

/// CfmBind is sendable over a channel
impl NonRequestSendable for CfmBind {
    fn wrap(self) -> ::Message {
        ::Message::Confirmation(::Confirmation::Http(HttpCfm::Bind(Box::new(self))))
    }
}

/// CfmResponseStart is sendable over a channel
impl NonRequestSendable for CfmResponseStart {
    fn wrap(self) -> ::Message {
        ::Message::Confirmation(::Confirmation::Http(HttpCfm::ResponseStart(Box::new(self))))
    }
}

/// CfmResponseBody is sendable over a channel
impl NonRequestSendable for CfmResponseBody {
    fn wrap(self) -> ::Message {
        ::Message::Confirmation(::Confirmation::Http(HttpCfm::ResponseBody(Box::new(self))))
    }
}

/// CfmResponseClose is sendable over a channel
impl NonRequestSendable for CfmResponseClose {
    fn wrap(self) -> ::Message {
        ::Message::Confirmation(::Confirmation::Http(HttpCfm::ResponseClose(Box::new(self))))
    }
}

/// IndConnected is sendable over a channel
impl NonRequestSendable for IndConnected {
    fn wrap(self) -> ::Message {
        ::Message::Indication(::Indication::Http(HttpInd::Connected(Box::new(self))))
    }
}

/// IndClosed is sendable over a channel
impl NonRequestSendable for IndClosed {
    fn wrap(self) -> ::Message {
        ::Message::Indication(::Indication::Http(HttpInd::Closed(Box::new(self))))
    }
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
