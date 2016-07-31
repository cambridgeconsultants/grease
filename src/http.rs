//! # http - The HTTP service
//!
//! This service depends on socket for the socket-y stuff, and
//! it uses the rushttp library to handle the HTTP parsing. The layer
//! above is responsible for working out what to do with the requests. This
//! layer will accept anything - OPTION, HEAD, GET, POST, etc - on any URL and
//! pass it on up. Consider this as a basic octet-to-IndConnected parser, and
//! a basic ReqResponseX-to-octet renderer. It's not a fully fledged webserver
//! by itself. The only thing we validate is the Host: header, so we know
//! which upper layer to pass the IndConnected on to.
//!
//! TODO: We need to support an IndBody / RspBody at some point, so that
//! POST and PUT will actually work.
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
//!
//! When an HTTP server is bound, we check our current bindings. If we are
//! not currently bound on that port, we bind it. Either way, we create a new
//! HttpServer object.
//!
//! When a new connection is received from the socket task, we create a new
//! HttpConnection object, which contains an HttpRequest parser object. When
//! data is received on the socket, it's passed to the parser and then we
//! respond to the indication to unblock the socket and allow more data in.
//! Once the parser returns something other than `ParseResult::InProgress`
//! we can check the request against our registered HttpServer objects
//! and pass up an IndConnected if appropriate (or reject the request with
//! an error if we don't like it).


// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use std::net;
use std::collections::HashMap;

use rushttp;
use ::socket;
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

/// A bind request - start an HTTP server on a given port.
#[derive(Debug)]
pub struct ReqBind {
    /// Which address to bind.
    pub addr: net::SocketAddr,
    /// Reflected back in the cfm, and in subsequent IndConnected
    pub context: ::Context,
    /// Send IndConnected here.
    pub ind_to: ::MessageSender,
}

/// Send the headers for an HTTP response. Host, Content-Length
/// and Content-Type are automatically added from the relevant fields
/// but you can add arbitrary other headers in the header vector.
#[derive(Debug)]
pub struct ReqResponseStart {
    /// Which HTTP connection to start a response on
    pub handle: ConnectionHandle,
    /// Reflected back in the cfm
    pub context: ::Context,
    /// Content-type for the response, e.g. "text/html"
    pub content_type: String,
    /// Length for the response - None means unbounded and 0 means no body.
    /// If there's no body, then ReqResponseClose should not be sent as it's implied.
    pub length: Option<usize>,
    /// Any other headers required.
    pub headers: HashMap<String, String>,
}

/// Send some body content for an HTTP response. Must be proceeded
/// by ReqResponseStart to send the headers.
#[derive(Debug)]
pub struct ReqResponseBody {
    /// Which HTTP connection to send some response body on
    pub handle: ConnectionHandle,
    /// Reflected back in the cfm
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
    pub result: Result<ServerHandle, HttpError>,
}

/// Whether the ReqResponseStart was successfull
#[derive(Debug)]
pub struct CfmResponseStart {
    pub handle: ConnectionHandle,
    pub context: ::Context,
    pub result: Result<(), HttpError>,
}

/// Confirms a ReqResponseBody has been sent
#[derive(Debug)]
pub struct CfmResponseBody {
    pub handle: ConnectionHandle,
    pub context: ::Context,
    pub result: Result<(), HttpError>,
}

/// Confirms the connection has been closed
#[derive(Debug)]
pub struct CfmResponseClose {
    pub handle: ConnectionHandle,
    pub context: ::Context,
    pub result: Result<(), HttpError>,
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
#[derive(Debug, Copy, Clone)]
pub enum HttpError {
    /// Used when I'm writing code and haven't added the correct error yet
    Unknown,
    /// Used if a ReqResponseXXX is sent on an invalid (perhaps recently
    /// closed) ConnectionHandle
    BadHandle,
    /// Socket bind failed,
    SocketError(socket::SocketError),
}

// ****************************************************************************
//
// Private Types
//
// ****************************************************************************

struct ReplyContext {
    pub reply_to: ::MessageSender,
    pub context: ::Context,
}

struct HttpServer {
    /// Supplied in a `socket::CfmBind`
    pub listen_handle: Option<socket::ListenHandle>,
    /// For reference, which socket address this is on
    pub addr: net::SocketAddr,
    /// To whom we need to acknowledge the bind/unbind
    pub reply_ctx: Option<ReplyContext>,
    /// The handle by which the upper layer refers to us
    pub our_handle: ServerHandle,
    /// Who to tell about the new connections we get
    pub ind_to: ::MessageSender,
    /// The connections we have
    pub connections: Vec<HttpConnection>,
}

struct HttpConnection {
    /// The socket handle for this specific connection
    pub conn_handle: socket::ConnectedHandle,
    /// The parser object we feed data through
    pub parser: rushttp::http_request::HttpRequestParser,
}

struct TaskContext {
    /// Who we send socket messages to
    socket: ::MessageSender,
    /// How other tasks send messages to us
    reply_to: ::MessageSender,
    /// Our list of servers
    servers: HashMap<::Context, HttpServer>,
    /// The next context we use for downward messages
    next_ctx: ::Context,
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
pub fn make_task(socket: &::MessageSender) -> ::MessageSender {
    let local_socket = socket.clone();
    ::make_task("http",
                move |rx: ::MessageReceiver, tx: ::MessageSender| main_loop(rx, tx, local_socket))
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
fn main_loop(rx: ::MessageReceiver, tx: ::MessageSender, socket: ::MessageSender) {
    let mut t = TaskContext::new(socket, tx);
    loop {
        if let Ok(msg) = rx.recv() {
            t.handle(msg);
        } else {
            break;
        }
    }
    panic!("This task should never die!");
}

/// All our handler functions are methods on this TaskContext structure.
impl TaskContext {
    /// Create a new TaskContext
    fn new(socket: ::MessageSender, us: ::MessageSender) -> TaskContext {
        TaskContext {
            socket: socket,
            servers: HashMap::new(),
            reply_to: us,
            // This number is arbitrary
            next_ctx: 2_000,
        }
    }

    /// Get a new context number, which doesn't match any of the previous
    /// context numbers (unless we've been running for a really really long time).
    fn get_ctx(&mut self) -> ::Context {
        let result = self.next_ctx;
        self.next_ctx = self.next_ctx + 1;
        result
    }

    /// Handle an incoming message. It might a `Request` for us,
    /// or it might be a `Confirmation` or `Indication` from a lower layer.
    fn handle(&mut self, msg: ::Message) {
        match msg {
            // We only handle our own requests and responses
            ::Message::Request(ref reply_to, ::Request::Http(ref x)) => {
                self.handle_http_req(x, reply_to)
            }
            ::Message::Request(ref reply_to, ::Request::Generic(ref x)) => {
                self.handle_generic_req(x, reply_to)
            }
            // We use the socket task so we expect to get confirmations and indications from it
            ::Message::Confirmation(::Confirmation::Socket(ref x)) => self.handle_socket_cfm(x),
            ::Message::Indication(::Indication::Socket(ref x)) => self.handle_socket_ind(x),
            // If we get here, someone else has made a mistake
            _ => error!("Unexpected message in http task: {:?}", msg),
        }
    }

    /// Handle an incoming `Confirmation` from the socket task
    fn handle_socket_cfm(&mut self, msg: &socket::SocketCfm) {
        match *msg {
            socket::SocketCfm::Bind(ref x) => self.handle_socket_bind(x),
            socket::SocketCfm::Unbind(ref x) => self.handle_socket_unbind(x),
            socket::SocketCfm::Close(ref x) => self.handle_socket_close(x),
            socket::SocketCfm::Send(ref x) => self.handle_socket_send(x),
        }
    }

    /// Handle a response to a socket task bind request. It's either
    /// bound our socket, or it failed, but either way we must let
    /// our user know.
    fn handle_socket_bind(&mut self, msg: &socket::CfmBind) {
        if let Some(ref mut server) = self.servers.get_mut(&msg.context) {
            if let Some(ref reply_ctx) = server.reply_ctx {
                match msg.result {
                    Ok(ref handle) => {
                        server.listen_handle = Some(*handle);
                        let cfm = CfmBind {
                            context: reply_ctx.context,
                            result: Ok(server.our_handle),
                        };
                        reply_ctx.reply_to.send_message(cfm);
                    }
                    Err(ref err) => {
                        let cfm = CfmBind {
                            context: reply_ctx.context,
                            result: Err(HttpError::SocketError(*err)),
                        };
                        reply_ctx.reply_to.send_message(cfm);
                    }
                }
            } else {
                warn!("Got CfmBind but no reply_ctx. Ignoring.");
            }
            server.reply_ctx = None
        } else {
            warn!("Context {} not found", msg.context);
        }
    }

    fn handle_socket_unbind(&mut self, _: &socket::CfmUnbind) {}

    fn handle_socket_close(&mut self, _: &socket::CfmClose) {}

    fn handle_socket_send(&mut self, _: &socket::CfmSend) {}

    fn handle_socket_ind(&mut self, _: &socket::SocketInd) {}

    fn handle_http_req(&mut self, msg: &HttpReq, reply_to: &::MessageSender) {
        match *msg {
            HttpReq::Bind(ref x) => self.handle_bind(x, reply_to),
            HttpReq::ResponseStart(ref x) => self.handle_responsestart(x, reply_to),
            HttpReq::ResponseBody(ref x) => self.handle_responsebody(x, reply_to),
            HttpReq::ResponseClose(ref x) => self.handle_responseclose(x, reply_to),
        }

    }

    fn handle_bind(&mut self, msg: &ReqBind, reply_to: &::MessageSender) {
        let reply_ctx = ReplyContext {
            context: msg.context,
            reply_to: reply_to.clone(),
        };
        let server = HttpServer {
            listen_handle: None,
            addr: msg.addr,
            reply_ctx: Some(reply_ctx),
            our_handle: self.get_ctx(),
            ind_to: msg.ind_to.clone(),
            connections: Vec::new(),
        };
        let req = socket::ReqBind {
            addr: msg.addr,
            context: server.our_handle,
        };
        self.socket.send_request(req, &self.reply_to);
        self.servers.insert(server.our_handle, server);
    }

    fn handle_responsestart(&mut self, msg: &ReqResponseStart, reply_to: &::MessageSender) {
        debug!("handle_responsestart: {:?}", msg);
        let cfm = CfmResponseStart {
            context: msg.context,
            handle: msg.handle,
            result: Err(HttpError::Unknown),
        };
        reply_to.send_message(cfm);
    }

    fn handle_responsebody(&mut self, msg: &ReqResponseBody, reply_to: &::MessageSender) {
        debug!("handle_responsebody: {:?}", msg);
        let cfm = CfmResponseBody {
            context: msg.context,
            handle: msg.handle,
            result: Err(HttpError::Unknown),
        };
        reply_to.send_message(cfm);
    }

    fn handle_responseclose(&mut self, msg: &ReqResponseClose, reply_to: &::MessageSender) {
        debug!("handle_responseclose: {:?}", msg);
        let cfm = CfmResponseClose {
            context: msg.context,
            handle: msg.handle,
            result: Err(HttpError::Unknown),
        };
        reply_to.send_message(cfm);
    }

    fn handle_generic_req(&mut self, msg: &::GenericReq, reply_to: &::MessageSender) {
        match *msg {
            ::GenericReq::Ping(ref x) => {
                let cfm = ::PingCfm { context: x.context };
                reply_to.send_message(cfm);
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
