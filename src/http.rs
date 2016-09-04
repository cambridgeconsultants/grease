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
//! socket task. If that succeeds, a new Server object is created.
//!
//! When an `IndConnected` is received from the socket task, we create a new
//! `Connection` object, which contains a parser (amongst other things). When
//! data is received on the socket, it's passed to the parser and then we
//! respond to the socket task to unblock the socket and allow more data in.
//! Once the parser is satisfied, we can check the decoded HTTP request
//! against our registered Server objects and pass up an `IndConnected` if
//! appropriate (or reject the request with an HTTP error response if we don't
//! like it).
//!
//! After an `IndRequest` has been sent up, an `IndClosed` will be sent when
//! the connection subsequently closes (perhaps prematurely). The user of the
//! service should follow an `IndConnected` with a `ReqResponseStart` then
//! zero or more `ReqResponseBody`. For flow control it is recommended that
//! the user waits for the `CfmResponseBody` before sending another
//! `ReqResponseBody`. Although the socket task underneath should buffer all
//! the data anyway, using flow control properly saves memory - especially
//! when sending large bodies.
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

pub use rushttp::http_response::HttpResponseStatus;

// ****************************************************************************
//
// Public Messages
//
// ****************************************************************************

/// Requests that can be sent to the http task.
#[derive(Debug)]
pub enum Request {
	/// A bind request - start an HTTP server on a given port
	Bind(Box<ReqBind>),
	/// Send the headers for an HTTP response
	ResponseStart(Box<ReqResponseStart>),
	/// Send some body content for an HTTP response
	ResponseBody(Box<ReqResponseBody>),
}

/// Confirmations that must be sent back to the http task.
#[derive(Debug)]
pub enum Confirmation {
	/// Whether the ReqBind was successfull
	Bind(Box<CfmBind>),
	/// Whether the ReqResponseStart was successfull
	ResponseStart(Box<CfmResponseStart>),
	/// Confirms a ReqResponseBody has been sent
	ResponseBody(Box<CfmResponseBody>),
}

/// Indications that come out of the http task.
#[derive(Debug)]
pub enum Indication {
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

make_request!(ReqBind, ::Request::Http, Request::Bind);

/// Send the headers for an HTTP response. Host, Content-Length
/// and Content-Type are automatically added from the relevant fields
/// but you can add arbitrary other headers in the header vector.
///
/// Send zero or more ReqResponseBody to fulfill the length specified.
/// (Wait for CfmResponseBody between each one as the link might be slow).
/// If unbounded length, send an empty ReqResponseBody to finish so we
/// know to close the connection.
#[derive(Debug)]
pub struct ReqResponseStart {
	/// Which HTTP connection to start a response on
	pub handle: ConnHandle,
	/// Reflected back in the cfm
	pub context: ::Context,
	/// Status code
	pub status: HttpResponseStatus,
	/// Content-Type for the response, e.g. "text/html"
	pub content_type: String,
	/// Length for the response - None means unbounded and 0 means no body.
	/// If not Some(0), then send some ReqResponseBody next.
	pub length: Option<usize>,
	/// Any other headers required.
	pub headers: HashMap<String, String>,
}

make_request!(ReqResponseStart, ::Request::Http, Request::ResponseStart);

/// Send some body content for an HTTP response. Must be proceeded
/// by ReqResponseStart to send the headers.
#[derive(Debug)]
pub struct ReqResponseBody {
	/// Which HTTP connection to send some response body on
	pub handle: ConnHandle,
	/// Reflected back in the cfm
	pub context: ::Context,
	/// Some data
	pub data: Vec<u8>,
}

make_request!(ReqResponseBody, ::Request::Http, Request::ResponseBody);

/// Whether the ReqBind was successfull
#[derive(Debug)]
pub struct CfmBind {
	pub context: ::Context,
	pub result: Result<ServerHandle, Error>,
}

make_confirmation!(CfmBind, ::Confirmation::Http, Confirmation::Bind);

/// Whether the ReqResponseStart was successfull
#[derive(Debug)]
pub struct CfmResponseStart {
	pub handle: ConnHandle,
	pub context: ::Context,
	pub result: Result<(), Error>,
}

make_confirmation!(CfmResponseStart,
				   ::Confirmation::Http,
				   Confirmation::ResponseStart);

/// Confirms a ReqResponseBody has been sent
#[derive(Debug)]
pub struct CfmResponseBody {
	pub handle: ConnHandle,
	pub context: ::Context,
	pub result: Result<(), Error>,
}

make_confirmation!(CfmResponseBody,
				   ::Confirmation::Http,
				   Confirmation::ResponseBody);

/// A new HTTP request has been received
#[derive(Debug)]
pub struct IndConnected {
	pub server_handle: ServerHandle,
	pub connection_handle: ConnHandle,
	pub url: String,
	pub method: rushttp::http::HttpMethod,
	pub headers: HashMap<String, String>,
}

make_indication!(IndConnected, ::Indication::Http, Indication::Connected);

/// An HTTP connection has been dropped
#[derive(Debug)]
pub struct IndClosed {
	pub handle: ConnHandle,
}

make_indication!(IndClosed, ::Indication::Http, Indication::Closed);

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Users of the http task should implement this trait to
/// make handling the incoming Confirmation and Indication a little
/// easier.
pub trait User {
	/// Handles a Http Confirmation, such as you will receive after sending
	/// a Http Request, by unpacking the enum and routing the struct
	/// contained within to the appropriate handler.
	fn handle_http_cfm(&mut self, msg: &Confirmation) {
		match *msg {
			Confirmation::Bind(ref x) => self.handle_http_cfm_bind(&x),
			Confirmation::ResponseStart(ref x) => self.handle_http_cfm_response_start(&x),
			Confirmation::ResponseBody(ref x) => self.handle_http_cfm_response_body(&x),
		}
	}

	/// Called when a Bind confirmation is received.
	fn handle_http_cfm_bind(&mut self, msg: &CfmBind);

	/// Called when an ResponseStart confirmation is received.
	fn handle_http_cfm_response_start(&mut self, msg: &CfmResponseStart);

	/// Called when a ResponseBody confirmation is received.
	fn handle_http_cfm_response_body(&mut self, msg: &CfmResponseBody);

	/// Handles a Http Indication by unpacking the enum and routing the
	/// struct contained withing to the appropriate handler.
	fn handle_http_ind(&mut self, msg: &Indication) {
		match *msg {
			Indication::Connected(ref x) => self.handle_http_ind_connected(&x),
			Indication::Closed(ref x) => self.handle_http_ind_closed(&x),
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
pub type ConnHandle = ::Context;

/// All possible http task errors
#[derive(Debug, Copy, Clone)]
pub enum Error {
	/// Used when I'm writing code and haven't added the correct error yet
	Unknown,
	/// Used if a ReqResponseXXX is sent on an invalid (perhaps recently
	/// closed) ConnHandle
	BadHandle,
	/// Socket bind failed,
	SocketError(socket::SocketError),
}

// ****************************************************************************
//
// Public Data
//
// ****************************************************************************

// None

// ****************************************************************************
//
// Private Types
//
// ****************************************************************************

enum CfmType {
	Start,
	Body,
	Close
}

struct PendingCfm {
	reply_to: ::MessageSender,
	their_context: ::Context,
	cfm_type: CfmType,
	handle: ConnHandle,
}

struct Server {
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

struct Connection {
	/// The handle by which the upper layer refers to us
	pub our_handle: ConnHandle,
	/// The server we were spawned for
	pub server_handle: ServerHandle,
	/// The socket handle for this specific connection
	pub socket_handle: socket::ConnHandle,
	/// The parser object we feed data through
	pub parser: rushttp::http_request::HttpRequestParser,
	/// Cfms we haven't sent yet, indexed by the unique context ID we
	/// send to the socket task.
	pub pending: HashMap<::Context, PendingCfm>,
}

struct TaskContext {
	/// Who we send socket messages to
	socket: ::MessageSender,
	/// How other tasks send messages to us
	reply_to: ::MessageSender,
	/// Our list of servers, indexed by the handle given in CfmBind
	servers: MultiMap<ServerHandle, Option<socket::ListenHandle>, Server>,
	/// Our list of connections, indexed by the handle given in IndConnected
	connections: MultiMap<ConnHandle, socket::ConnHandle, Connection>,
	/// The next context we use for downward messages
	next_ctx: ::Context,
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
	::make_task("http",
				move |rx: ::MessageReceiver, tx: ::MessageSender| main_loop(rx, tx, local_socket))
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

/// The task runs this main loop indefinitely.
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
	/// context numbers (unless we've been running for a really really long
	/// time).
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
			// We use the socket task so we expect to get confirmations and indications from it
			::Message::Confirmation(::Confirmation::Socket(ref x)) => self.handle_socket_cfm(x),
			::Message::Indication(::Indication::Socket(ref x)) => self.handle_socket_ind(x),
			// If we get here, someone else has made a mistake
			_ => error!("Unexpected message in http task: {:?}", msg),
		}
	}

	fn handle_http_req(&mut self, req: &Request, reply_to: &::MessageSender) {
		match *req {
			Request::Bind(ref x) => self.handle_bind(x, reply_to),
			Request::ResponseStart(ref x) => self.handle_responsestart(x, reply_to),
			Request::ResponseBody(ref x) => self.handle_responsebody(x, reply_to),
		}
	}

	fn handle_bind(&mut self, req_bind: &ReqBind, reply_to: &::MessageSender) {
		let reply_ctx = ::ReplyContext {
			context: req_bind.context,
			reply_to: reply_to.clone(),
		};
		let server = Server {
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

	/// Get the connection from a connection handle
	fn get_conn_by_http_handle(&mut self, handle: &ConnHandle) -> Option<&mut Connection> {
		self.connections.get_mut(handle)
	}

	/// Remove the connection from a connection handle
	fn remove_conn_by_http_handle(&mut self, handle: &ConnHandle) -> Option<&mut Connection> {
		self.connections.remove(handle)
	}

	/// Get the Server for a given socket ListenHandle.
	fn get_server_by_socket_handle(&mut self,
								   handle: &socket::ListenHandle)
								   -> Option<&mut Server> {
		// The key here is Option<socket::ListenHandle> because
		// it takes time to bind the socket.
		self.servers.get_mut_alt(&Some(*handle))
	}

	/// Get the ServerHandle for a given socket ListenHandle. Used when
	/// we get a socket IndConnected.
	fn get_server_handle_by_socket_handle(&mut self,
										  handle: &socket::ListenHandle)
										  -> Option<ServerHandle> {
		self.get_server_by_socket_handle(handle).and_then(|x| Some(x.our_handle))
	}

	/// Get a reference to the appropriate Connection object, and its
	/// associated Server object. If one or the other isn't found, return
	/// None.
	fn get_conn_by_socket_handle(&mut self,
								 handle: &socket::ConnHandle)
								 -> Option<(&mut Connection, &mut Server)> {
		let c = self.connections.get_mut_alt(handle);
		if let Some(conn) = c {
			if let Some(serv) = self.servers.get_mut(&conn.server_handle) {
				return Some((conn, serv));
			}
		}
		None
	}

	fn delete_connection_by_socket_handle(&mut self, handle: &socket::ConnHandle) {
		self.connections.remove_alt(handle);
	}

	fn render_response(
		status: HttpResponseStatus,
		content_type: &str,
		length: Option<usize>,
		headers: &HashMap<String, String>) -> String
	{
		let mut s = String::new();
		s.push(format!("HTTP/1.1 {}\r\n", status));
		if !headers.contains("Server") {
			s.push("Server: grease/http\r\n");
		}
		if !headers.contains("Content-Length") {
			if let Some(length) = length {
				s.push(format!("Content-Length: {}\r\n", length));
			}
		}
        for (k, v) in &headers {
            s.push(format!("{}: {}\r\n", k, v));
		}
		s.push("\r\n");
		return s;
	}

	fn handle_responsestart(&mut self, req_start: &ReqResponseStart, reply_to: &::MessageSender) {
		if let Some(conn) = self.get_conn_by_http_handle(req_start.handle) {
			// Render the headers as a String
			// Send to the socket server
			// Send the cfm when the socket server has sent this data
			let s = TaskContext::render_response(req_start.status, req_start.content_type, req_start.length, &req_start.headers);
			let req = socket::ReqSend {
				handle: conn.socket_handle,
				context: self.get_ctx(),
				data: s.to_bytes()
			};
			let pend = PendingCfm {
				their_context: req_start.context,
				reply_to: reply_to.clone(),
				cfm_type: CfmType::Start,
			};
			self.pending.insert(req.context, pend);
			self.socket.send_request(req, &self.reply_to);
		}
		else {
			debug!("handle_responsestart: {:?}", req_start);
			let cfm = CfmResponseStart {
				context: req_start.context,
				handle: req_start.handle,
				result: Err(Error::BadHandle),
			};
			reply_to.send_nonrequest(cfm);
		}
	}

	fn handle_responsebody(&mut self, req_body: &ReqResponseBody, reply_to: &::MessageSender) {
		// @todo send body here
		let cfm = CfmResponseBody {
			context: req_body.context,
			handle: req_body.handle,
			result: Err(Error::Unknown),
		};
		reply_to.send_nonrequest(cfm);
	}

	fn send_response(&self,
					 handle: &socket::ConnHandle,
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

	fn map_result<T>(result: &Result<T, socket::SocketError>) -> Result<(), Error> {
		match result {
			Ok(_) => Ok(()),
			Err(e) => Err(Error::SocketError(e))
		}
	}

	fn handle_socket_cfm_send(&mut self, cfm: &socket::CfmSend) {
		debug!("Got {:?}", cfm);
		if let Some(pend) = self.pending.remove(cfm.context) {
			match pend.cfm_type {
				CfmType::Start => {
					let msg = CfmResponseStart {
						context: pend.context,
						handle: pend.handle,
						result: TaskContext::map_result(cfm.result),
					};
					pend.reply_to.send_nonrequest(msg);
				}
				CfmType::Body => {
					let msg = CfmResponseBody {
						context: pend.context,
						handle: pend.handle,
						result: TaskContext::map_result(cfm.result),
					};
					pend.reply_to.send_nonrequest(msg);
				}
				CfmType::Close => {
					panic!("CfmSend with close context?");
				}
			}
		}
	}

	/// Someone has connected to our bound socket, so create a Connection
	/// and wait for data. We don't tell the layer above until we've parsed
	/// the HTTP Request header.
	fn handle_socket_ind_connected(&mut self, ind: &socket::IndConnected) {
		debug!("Got {:?}", ind);
		if let Some(server_handle) = self.get_server_handle_by_socket_handle(&ind.listen_handle) {
			let conn = Connection {
				our_handle: self.get_ctx(),
				server_handle: server_handle,
				socket_handle: ind.conn_handle,
				parser: rushttp::http_request::HttpRequestParser::new(),
			};
			debug!("New connection {:?}, socket={:?}",
				   conn.our_handle,
				   conn.socket_handle);
			self.connections.insert(conn.our_handle, conn.socket_handle, conn);
		} else {
			warn!("Connection on non-existant socket handle");
		}
	}

	fn handle_socket_ind_dropped(&mut self, ind: &socket::IndDropped) {
		self.connections.remove_alt(&ind.handle);
	}

	fn handle_socket_ind_received(&mut self, ind: &socket::IndReceived) {
		debug!("Got {:?}", ind);
		let r = if let Some((conn, serv)) = self.get_conn_by_socket_handle(&ind.handle) {
			debug!("Got data for conn {:?}!", conn.our_handle);
			// Extract the fields from
			Some((conn.parser.parse(&ind.data),
				  conn.our_handle,
				  conn.server_handle,
				  serv.ind_to.clone()))
		} else {
			None
		};

		match r {
			Some((rushttp::http_request::ParseResult::Complete(req, len), ch, sh, ind_to)) => {
				// All done!
				let conn_ind = IndConnected {
					server_handle: sh,
					connection_handle: ch,
					url: req.url,
					method: req.method,
					headers: req.headers,
				};
				ind_to.send_nonrequest(conn_ind);
			}
			Some((rushttp::http_request::ParseResult::InProgress, _, _, _)) => {
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
