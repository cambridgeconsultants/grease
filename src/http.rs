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
use multi_map::MultiMap;

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

struct HttpServer {
    /// Supplied in a `socket::CfmBind`
    pub listen_handle: Option<socket::ListenHandle>,
    /// For reference, which socket address this is on
    pub addr: net::SocketAddr,
    /// To whom we need to acknowledge the bind/unbind
    pub reply_ctx: Option<::ReplyContext>,
    /// The handle by which the upper layer refers to us
    pub our_handle: ServerHandle,
    /// Who to tell about the new connections we get
    pub ind_to: ::MessageSender,
}

struct HttpConnection {
    /// The handle by which the upper layer refers to us
    pub our_handle: ConnectionHandle,
    /// The server we were spawned for
    pub server_handle: ServerHandle,
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
    /// Our list of servers, indexed by the handle given in CfmBind
    servers: MultiMap<ServerHandle, Option<socket::ListenHandle>, HttpServer>,
    /// Our list of connections, indexed by the handle given in IndConnected
    connections: MultiMap<ConnectionHandle, socket::ConnectedHandle, HttpConnection>,
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
    for msg in rx.iter() {
        t.handle(msg);
    }
    panic!("This task should never die!");
}

/// All our handler functions are methods on this TaskContext structure.
impl TaskContext {
    /// Create a new TaskContext
    fn new(socket: ::MessageSender, us: ::MessageSender) -> TaskContext {
        TaskContext {
            socket: socket,
            servers: MultiMap::new(),
            connections: MultiMap::new(),
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
    fn handle_socket_cfm(&mut self, cfm: &socket::SocketCfm) {
        match *cfm {
            socket::SocketCfm::Bind(ref x) => self.handle_socket_cfm_bind(x),
            socket::SocketCfm::Unbind(ref x) => self.handle_socket_cfm_unbind(x),
            socket::SocketCfm::Close(ref x) => self.handle_socket_cfm_close(x),
            socket::SocketCfm::Send(ref x) => self.handle_socket_cfm_send(x),
        }
    }

    /// Handle a response to a socket task bind request. It's either
    /// bound our socket, or it failed, but either way we must let
    /// our user know.
    fn handle_socket_cfm_bind(&mut self, cfm_bind: &socket::CfmBind) {
        if let Some(mut server) = self.servers.remove(&cfm_bind.context) {
            let reply_ctx = server.reply_ctx.unwrap();
            server.reply_ctx = None;
            match cfm_bind.result {
                Ok(ref handle) => {
                    server.listen_handle = Some(*handle);
                    let cfm = CfmBind {
                        context: reply_ctx.context,
                        result: Ok(server.our_handle),
                    };
                    // Re-insert, but with socket handle as second key
                    reply_ctx.reply_to.send_nonrequest(cfm);
                    self.servers.insert(cfm_bind.context, Some(*handle), server);
                }
                Err(ref err) => {
                    let cfm = CfmBind {
                        context: reply_ctx.context,
                        result: Err(HttpError::SocketError(*err)),
                    };
                    reply_ctx.reply_to.send_nonrequest(cfm);
                }
            }
        } else {
            warn!("Context {} not found", cfm_bind.context);
        }
    }

    fn handle_socket_cfm_unbind(&mut self, cfm: &socket::CfmUnbind) {
        debug!("Got {:?}", cfm);
    }

    fn handle_socket_cfm_close(&mut self, cfm: &socket::CfmClose) {
        debug!("Got {:?}", cfm);
    }

    fn handle_socket_cfm_send(&mut self, cfm: &socket::CfmSend) {
        debug!("Got {:?}", cfm);
    }

    fn handle_socket_ind(&mut self, ind: &socket::SocketInd) {
        match *ind {
            socket::SocketInd::Connected(ref x) => self.handle_socket_ind_connected(x),
            socket::SocketInd::Dropped(ref x) => self.handle_socket_ind_dropped(x),
            socket::SocketInd::Received(ref x) => self.handle_socket_ind_received(x),
        }
    }

    fn handle_socket_ind_connected(&mut self, ind: &socket::IndConnected) {
        debug!("Got {:?}", ind);
        let mut server_handle = None;
        if let Some(ref server) = self.get_server_by_socket_handle(ind.listen_handle) {
            server_handle = Some(server.our_handle);
        }
        if server_handle.is_some() {
            debug!("New connection on {:?}!", server_handle);
            let conn = HttpConnection {
                our_handle: self.get_ctx(),
                server_handle: server_handle.unwrap(),
                conn_handle: ind.open_handle,
                parser: rushttp::http_request::HttpRequestParser::new(),
            };
            self.connections.insert(conn.our_handle, conn.conn_handle, conn);
        } else {
            warn!("Connection on non-existant socket handle");
        }
    }

    fn handle_socket_ind_dropped(&mut self, ind: &socket::IndDropped) {
        self.connections.remove_alt(&ind.handle);
    }

    fn handle_socket_ind_received(&mut self, ind: &socket::IndReceived) {
        debug!("Got {:?}", ind);
        if let Some(ref mut conn) = self.get_conn_by_socket_handle(ind.handle) {
            debug!("Got data for conn {:?}!", conn.our_handle);
        } else {
            warn!("Drop on non-existant socket handle");
        }
    }

    fn handle_http_req(&mut self, req: &HttpReq, reply_to: &::MessageSender) {
        match *req {
            HttpReq::Bind(ref x) => self.handle_bind(x, reply_to),
            HttpReq::ResponseStart(ref x) => self.handle_responsestart(x, reply_to),
            HttpReq::ResponseBody(ref x) => self.handle_responsebody(x, reply_to),
            HttpReq::ResponseClose(ref x) => self.handle_responseclose(x, reply_to),
        }
    }

    fn handle_bind(&mut self, req_bind: &ReqBind, reply_to: &::MessageSender) {
        let reply_ctx = ::ReplyContext {
            context: req_bind.context,
            reply_to: reply_to.clone(),
        };
        let server = HttpServer {
            listen_handle: None,
            addr: req_bind.addr,
            reply_ctx: Some(reply_ctx),
            our_handle: self.get_ctx(),
            ind_to: reply_to.clone(),
        };
        let req = socket::ReqBind {
            addr: req_bind.addr,
            context: server.our_handle,
        };
        self.socket.send_request(req, &self.reply_to);
        self.servers.insert(server.our_handle, None, server);
    }

    fn get_server_by_socket_handle(&mut self, handle: socket::ListenHandle) -> Option<&HttpServer> {
        self.servers.get_alt(&Some(handle))
    }

    fn get_conn_by_socket_handle(&mut self,
                                 handle: socket::ConnectedHandle)
                                 -> Option<&HttpConnection> {
        self.connections.get_alt(&handle)
    }

    fn handle_responsestart(&mut self, req_start: &ReqResponseStart, reply_to: &::MessageSender) {
        debug!("handle_responsestart: {:?}", req_start);
        let cfm = CfmResponseStart {
            context: req_start.context,
            handle: req_start.handle,
            result: Err(HttpError::Unknown),
        };
        reply_to.send_nonrequest(cfm);
    }

    fn handle_responsebody(&mut self, req_body: &ReqResponseBody, reply_to: &::MessageSender) {
        debug!("handle_responsebody: {:?}", req_body);
        let cfm = CfmResponseBody {
            context: req_body.context,
            handle: req_body.handle,
            result: Err(HttpError::Unknown),
        };
        reply_to.send_nonrequest(cfm);
    }

    fn handle_responseclose(&mut self, req_close: &ReqResponseClose, reply_to: &::MessageSender) {
        debug!("handle_responseclose: {:?}", req_close);
        let cfm = CfmResponseClose {
            context: req_close.context,
            handle: req_close.handle,
            result: Err(HttpError::Unknown),
        };
        reply_to.send_nonrequest(cfm);
    }

    fn handle_generic_req(&mut self, req: &::GenericReq, reply_to: &::MessageSender) {
        match *req {
            ::GenericReq::Ping(ref x) => {
                let cfm = ::PingCfm { context: x.context };
                reply_to.send_nonrequest(cfm);
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
