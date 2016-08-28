//! # http - An HTTP parsing/rendering task
//!
//! This task implements a basic HTTP parser/renderer. It depends on the
//! `socket` task to interface with the network, and passes up decoded HTTP
//! requests. In reply, it receives requests to send the HTTP response.
//!
//! The layer above is responsible for working out what to do with the HTTP
//! requests. This layer will accept anything - OPTION, HEAD, GET, POST, etc -
//! on any URL and pass it on up. It is an HTTP request parser and HTTP
//! response renderer as opposed to a fully fledged web-server, but it might
//! be useful it you wanted to implement a web server.
//!
//! When a `ReqBind` is received, it attempts to bind a new socket with the
//! socket task. If that succeeds, a new HttpServer object is created.
//!
//! When an `IndConnected` is received from the socket task, we create a new
//! `HttpConnection` object, which contains a parser (amongst other things).
//! When data is received on the socket, it's passed to the parser and then we
//! respond to the socket task to unblock the socket and allow more data in.
//! Once the parser is satisfied, we can check the decoded HTTP request against our
//! registered HttpServer objects and pass up an `IndConnected` if appropriate
//! (or reject the request with an HTTP error response if we don't like it).
//!
//! After an `IndRequest` has been sent up, an `IndClosed` will be sent when
//! the connection subsequently closes (perhaps prematurely). The user of the
//! service should follow an `IndConnected` with a `ReqResponseStart`, zero or
//! more `ReqResponseBody` and then a `ReqResponseClose`. For flow control it
//! is recommended that the user waits for the `CfmResponseBody` before
//! sending another `ReqResponseBody`. Although the socket task underneath
//! should buffer all the data anyway, using flow control properly saves
//! memory - especially when sending large bodies.
//!
//! TODO: We need to support an IndBody / RspBody at some point, so that
//! POST and PUT will actually work.

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use std::collections::HashMap;
use std::net;

use multi_map::MultiMap;
use rushttp;
use ::socket;
use ::socket::User as SocketUser;
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

make_request!(ReqBind, ::Request::Http, HttpReq::Bind);

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

make_request!(ReqResponseStart, ::Request::Http, HttpReq::ResponseStart);


/// Send some body content for an HTTP response. Must be proceeded
/// by ReqResponseStart to send the headers.
#[derive(Debug)]
pub struct ReqResponseBody {
    /// Which HTTP connection to send some response body on
    pub handle: ConnectionHandle,
    /// Reflected back in the cfm
    pub context: ::Context,
}

make_request!(ReqResponseBody, ::Request::Http, HttpReq::ResponseBody);

/// Close out an HTTP response
#[derive(Debug)]
pub struct ReqResponseClose {
    pub handle: ConnectionHandle,
    pub context: ::Context,
}

make_request!(ReqResponseClose, ::Request::Http, HttpReq::ResponseClose);

/// Whether the ReqBind was successfull
#[derive(Debug)]
pub struct CfmBind {
    pub context: ::Context,
    pub result: Result<ServerHandle, Error>,
}

make_confirmation!(CfmBind, ::Confirmation::Http, HttpCfm::Bind);

/// Whether the ReqResponseStart was successfull
#[derive(Debug)]
pub struct CfmResponseStart {
    pub handle: ConnectionHandle,
    pub context: ::Context,
    pub result: Result<(), Error>,
}

make_confirmation!(CfmResponseStart,
                   ::Confirmation::Http,
                   HttpCfm::ResponseStart);

/// Confirms a ReqResponseBody has been sent
#[derive(Debug)]
pub struct CfmResponseBody {
    pub handle: ConnectionHandle,
    pub context: ::Context,
    pub result: Result<(), Error>,
}

make_confirmation!(CfmResponseBody, ::Confirmation::Http, HttpCfm::ResponseBody);

/// Confirms the connection has been closed
#[derive(Debug)]
pub struct CfmResponseClose {
    pub handle: ConnectionHandle,
    pub context: ::Context,
    pub result: Result<(), Error>,
}

make_confirmation!(CfmResponseClose,
                   ::Confirmation::Http,
                   HttpCfm::ResponseClose);

/// A new HTTP request has been received
#[derive(Debug)]
pub struct IndConnected {
    pub server_handle: ServerHandle,
    pub connection_handle: ConnectionHandle,
    pub url: String,
    pub method: rushttp::http::HttpMethod,
    pub headers: HashMap<String, String>,
}

make_indication!(IndConnected, ::Indication::Http, HttpInd::Connected);

/// An HTTP connection has been dropped
#[derive(Debug)]
pub struct IndClosed {
    pub handle: ConnectionHandle,
}

make_indication!(IndClosed, ::Indication::Http, HttpInd::Closed);

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Users of the http task should implement this trait to
/// make handling the incoming HttpCfm and HttpInd a little
/// easier.
pub trait User {
    /// Handles a Http Confirmation, such as you will receive after sending
    /// a Http Request, by unpacking the enum and routing the struct
    /// contained within to the appropriate handler.
    fn handle_socket_cfm(&mut self, msg: &HttpCfm) {
        match *msg {
            HttpCfm::Bind(ref x) => self.handle_http_cfm_bind(&x),
            HttpCfm::ResponseStart(ref x) => self.handle_http_cfm_response_start(&x),
            HttpCfm::ResponseBody(ref x) => self.handle_http_cfm_response_body(&x),
            HttpCfm::ResponseClose(ref x) => self.handle_http_cfm_response_close(&x),
        }
    }

    /// Called when a Bind confirmation is received.
    fn handle_http_cfm_bind(&mut self, msg: &CfmBind);

    /// Called when an ResponseStart confirmation is received.
    fn handle_http_cfm_response_start(&mut self, msg: &CfmResponseStart);

    /// Called when a ResponseBody confirmation is received.
    fn handle_http_cfm_response_body(&mut self, msg: &CfmResponseBody);

    /// Called when a ResponseClose confirmation is received.
    fn handle_http_cfm_response_close(&mut self, msg: &CfmResponseClose);

    /// Handles a Http Indication by unpacking the enum and routing the
    /// struct contained withing to the appropriate handler.
    fn handle_http_ind(&mut self, msg: &HttpInd) {
        match *msg {
            HttpInd::Connected(ref x) => self.handle_http_ind_connected(&x),
            HttpInd::Closed(ref x) => self.handle_http_ind_closed(&x),
        }
    }

    /// Handles a Connected indication.
    fn handle_http_ind_connected(&mut self, msg: &IndConnected);

    /// Handles a connection Closed indication.
    fn handle_http_ind_closed(&mut self, msg: &IndClosed);
}

/// A new one of these is allocated for every new HTTP server
pub type ServerHandle = ::Context;

/// A new one of these is allocated for every new HTTP request
pub type ConnectionHandle = ::Context;

/// All possible http task errors
#[derive(Debug, Copy, Clone)]
pub enum Error {
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

    fn get_server_by_socket_handle(&mut self,
                                   handle: socket::ListenHandle)
                                   -> Option<&mut HttpServer> {
        self.servers.get_mut_alt(&Some(handle))
    }

    fn get_conn_by_socket_handle(&mut self,
                                 handle: &socket::ConnectedHandle)
                                 -> Option<&mut HttpConnection> {
        self.connections.get_mut_alt(handle)
    }

    fn delete_connection_by_socket_handle(&mut self, handle: &socket::ConnectedHandle) {
        self.connections.remove_alt(handle);
    }

    fn handle_responsestart(&mut self, req_start: &ReqResponseStart, reply_to: &::MessageSender) {
        debug!("handle_responsestart: {:?}", req_start);
        let cfm = CfmResponseStart {
            context: req_start.context,
            handle: req_start.handle,
            result: Err(Error::Unknown),
        };
        reply_to.send_nonrequest(cfm);
    }

    fn handle_responsebody(&mut self, req_body: &ReqResponseBody, reply_to: &::MessageSender) {
        debug!("handle_responsebody: {:?}", req_body);
        let cfm = CfmResponseBody {
            context: req_body.context,
            handle: req_body.handle,
            result: Err(Error::Unknown),
        };
        reply_to.send_nonrequest(cfm);
    }

    fn handle_responseclose(&mut self, req_close: &ReqResponseClose, reply_to: &::MessageSender) {
        debug!("handle_responseclose: {:?}", req_close);
        let cfm = CfmResponseClose {
            context: req_close.context,
            handle: req_close.handle,
            result: Err(Error::Unknown),
        };
        reply_to.send_nonrequest(cfm);
    }

    fn send_response(&self,
                     handle: &socket::ConnectedHandle,
                     code: rushttp::http_response::HttpResponseStatus,
                     message: &str) {
        // An error occured which we must tell them about
        let mut r = rushttp::http_response::HttpResponse::new_with_body(code, "HTTP/1.0", message);
        r.add_header("Content-Type", "text/plain");
        let mut output = Vec::new();
        if let Ok(_) = r.write(&mut output) {
            let req = socket::ReqSend {
                handle: *handle,
                context: 0,
                data: output,
            };
            self.socket.send_request(req, &self.reply_to);
        } else {
            warn!("Failed to render error");
        }

        let close_req = socket::ReqClose {
            handle: *handle,
            context: 0,
        };
        self.socket.send_request(close_req, &self.reply_to);
    }
}

/// Handle generic requests.
impl GenericProvider for TaskContext {}

/// Socket specific handler methods
impl SocketUser for TaskContext {
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
                        result: Err(Error::SocketError(*err)),
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

    fn handle_socket_ind_connected(&mut self, ind: &socket::IndConnected) {
        debug!("Got {:?}", ind);
        let mut server_handle = None;
        if let Some(ref server) = self.get_server_by_socket_handle(ind.listen_handle) {
            server_handle = Some(server.our_handle);
        }
        if server_handle.is_some() {
            let conn = HttpConnection {
                our_handle: self.get_ctx(),
                server_handle: server_handle.unwrap(),
                conn_handle: ind.open_handle,
                parser: rushttp::http_request::HttpRequestParser::new(),
            };
            debug!("New connection {:?}, conn={:?}",
                   conn.our_handle,
                   ind.open_handle);
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
        let mut r = None;
        if let Some(ref mut conn) = self.get_conn_by_socket_handle(&ind.handle) {
            debug!("Got data for conn {:?}!", conn.our_handle);
            r = Some(conn.parser.parse(&ind.data));
        }

        match r {
            Some(rushttp::http_request::ParseResult::Complete(req, len)) => {
                // All done!
                // @todo this is debug. We should send an IndConnected here
                self.delete_connection_by_socket_handle(&ind.handle);
                let msg = format!("Received {} chars", len);
                self.send_response(&ind.handle,
                                   rushttp::http_response::HttpResponseStatus::OK,
                                   &msg);
            }
            Some(rushttp::http_request::ParseResult::InProgress) => {
                // Need more data
            }
            Some(_) => {
                self.delete_connection_by_socket_handle(&ind.handle);
                self.send_response(&ind.handle,
                                   rushttp::http_response::HttpResponseStatus::BadRequest,
                                   "Bad Request");
            }
            None => {
                warn!("Data on non-existant socket handle");
            }
        }

    }
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
