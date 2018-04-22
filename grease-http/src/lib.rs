//! # http - An HTTP parsing/rendering task
//!
//! Copyright (c) Cambridge Consultants 2018
//! See the top-level COPYRIGHT file for further information and licensing
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
//! When an `IndRxRequest` is received from the socket task, we create a new
//! `Connection` object, which contains a parser (amongst other things). When
//! data is received on the socket, it's passed to the parser and then we
//! respond to the socket task to unblock the socket and allow more data in.
//! Once the parser is satisfied, we can check the decoded HTTP request
//! against our registered Server objects and pass up an `IndRxRequest` if
//! appropriate (or reject the request with an HTTP error response if we don't
//! like it).
//!
//! After an `IndRequest` has been sent up, an `IndClosed` will be sent when
//! the connection subsequently closes (perhaps prematurely). The user of the
//! service should follow an `IndRxRequest` with a `ReqResponseStart` then
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

#[macro_use]
extern crate grease;
extern crate grease_socket as socket;
#[macro_use]
extern crate log;
extern crate multi_map;
extern crate rushttp;

use std::collections::HashMap;
use std::net;

use multi_map::MultiMap;
use std::sync::mpsc;

pub use rushttp::response::HttpResponseStatus;
pub use rushttp::{HeaderMap, Method, Uri};

use grease::Context;

// ****************************************************************************
//
// Public Messages
//
// ****************************************************************************

/// Offers the `grease::Service` for this module.
pub struct Service;

impl grease::Service for Service {
	type Request = Request;
	type Confirm = Confirm;
	type Indication = Indication;
	type Response = Response;
}

/// Requests that can be sent to the http task.
#[derive(Debug)]
pub enum Request {
	/// A bind request - start an HTTP server on a given port
	Bind(ReqBind),
	/// Send the headers for an HTTP response
	ResponseStart(ReqResponseStart),
	/// Send some body content for an HTTP response
	ResponseBody(ReqResponseBody),
}

make_wrapper!(ReqBind, Request, Request::Bind);
make_wrapper!(ReqResponseStart, Request, Request::ResponseStart);
make_wrapper!(ReqResponseBody, Request, Request::ResponseBody);

/// Confirms that must be sent back to the http task.
#[derive(Debug)]
pub enum Confirm {
	/// Whether the ReqBind was successfull
	Bind(CfmBind),
	/// Whether the ReqResponseStart was successfull
	ResponseStart(CfmResponseStart),
	/// Confirms a ReqResponseBody has been sent
	ResponseBody(CfmResponseBody),
}

make_wrapper!(CfmBind, Confirm, Confirm::Bind);
make_wrapper!(CfmResponseStart, Confirm, Confirm::ResponseStart);
make_wrapper!(CfmResponseBody, Confirm, Confirm::ResponseBody);

/// Indications that come out of the http task.
#[derive(Debug)]
pub enum Indication {
	/// A new HTTP request has been received
	RxRequest(IndRxRequest),
	/// An HTTP connection has been dropped
	Closed(IndClosed),
}

make_wrapper!(IndRxRequest, Indication, Indication::RxRequest);
make_wrapper!(IndClosed, Indication, Indication::Closed);

#[derive(Debug)]
pub enum Response {}

/// A bind request - start an HTTP server on a given port.
#[derive(Debug)]
pub struct ReqBind {
	/// Which address to bind.
	pub addr: net::SocketAddr,
	/// Reflected back in the cfm, and in subsequent IndRxRequest
	pub context: Context,
}

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
	pub context: Context,
	/// Status code
	pub status: HttpResponseStatus,
	/// Content-Type for the response, e.g. "text/html"
	pub content_type: String,
	/// Length for the response - None means unbounded and 0 means no body.
	/// If not Some(0), then send some ReqResponseBody next.
	pub length: Option<usize>,
	/// Any other headers required.
	pub headers: HeaderMap,
}

/// Send some body content for an HTTP response. Must be proceeded
/// by ReqResponseStart to send the headers.
#[derive(Debug)]
pub struct ReqResponseBody {
	/// Which HTTP connection to send some response body on
	pub handle: ConnHandle,
	/// Reflected back in the cfm
	pub context: Context,
	/// Some data
	pub data: Vec<u8>,
}

/// Whether the ReqBind was successfull
#[derive(Debug)]
pub struct CfmBind {
	pub context: Context,
	pub result: Result<ServerHandle, Error>,
}

/// Whether the ReqResponseStart was successfull
#[derive(Debug)]
pub struct CfmResponseStart {
	pub handle: ConnHandle,
	pub context: Context,
	pub result: Result<(), Error>,
}

/// Confirms a ReqResponseBody has been sent
#[derive(Debug)]
pub struct CfmResponseBody {
	pub handle: ConnHandle,
	pub context: Context,
	pub result: Result<(), Error>,
}

/// A new HTTP request has been received
#[derive(Debug)]
pub struct IndRxRequest {
	pub server_handle: ServerHandle,
	pub connection_handle: ConnHandle,
	pub url: Uri,
	pub method: Method,
	pub headers: HeaderMap,
}

/// An HTTP connection has been dropped
#[derive(Debug)]
pub struct IndClosed {
	pub handle: ConnHandle,
}

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Represents something an http service user can hold on to to send us
/// message.
pub struct Handle(mpsc::Sender<Incoming>);

/// A new one of these is allocated for every new HTTP server
pub type ServerHandle = Context;

/// A new one of these is allocated for every new HTTP request
pub type ConnHandle = Context;

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

service_map! {
	generate: Incoming,
	service: Service,
	handle: Handle,
	used: {
		socket: (Service, SocketCfm, SocketInd)
	}
}

enum CfmType {
	Start,
	Body,
	Close,
}

/// If we get a Request from above and consequently need to wait
/// for a Cfm from the socket task, use a PendingCfm to store
/// the details you'll need when the Cfm eventually arrives.
struct PendingCfm {
	reply_to: grease::ServiceUserHandle<Service>,
	context: Context,
	cfm_type: CfmType,
	handle: ConnHandle,
	/// If true, close the socket when this cfm comes in
	/// When the socket CfmClose comes in, send IndClosed.
	close_after: bool,
}

struct Server {
	/// Supplied in a `socket::CfmBind`
	listen_handle: Option<socket::ListenHandle>,
	/// To whom we need to acknowledge the bind/unbind
	reply_ctx: Option<ReplyContext>,
	/// The handle by which the upper layer refers to us
	our_handle: ServerHandle,
	/// Who to tell about the new connections we get
	ind_to: grease::ServiceUserHandle<Service>,
}

struct Connection {
	/// The handle by which the upper layer refers to us
	our_handle: ConnHandle,
	/// The server we were spawned for
	server_handle: ServerHandle,
	/// The socket handle for this specific connection
	socket_handle: socket::ConnHandle,
	/// The parser object we feed data through
	parser: rushttp::request::Parser,
	/// The length of the response body we're sending
	/// When enough has been sent, we close the connection automatically.
	/// If the length is None, close when an empty body request is sent
	/// If the length is Some(0), close after the headers
	body_length: Option<usize>,
}

struct TaskContext<T>
where
	T: grease::ServiceProvider<socket::Service>,
{
	/// Who we send socket messages to
	socket: T,
	/// How the tasks we use send messages to us
	reply_to: Handle,
	/// Our list of servers, indexed by the handle given in CfmBind
	servers: MultiMap<ServerHandle, Option<socket::ListenHandle>, Server>,
	/// Our list of connections, indexed by the handle given in IndRxRequest
	connections: MultiMap<ConnHandle, socket::ConnHandle, Connection>,
	/// The next context we use for downward messages
	next_ctx: Context,
	/// Cfms we haven't sent yet, indexed by the unique context ID we
	/// send to the socket task.
	pending: HashMap<Context, PendingCfm>,
}

type ReplyContext = grease::ReplyContext<Service>;

// ****************************************************************************
//
// Public Functions
//
// ****************************************************************************

/// Creates a new socket task. Returns an object that can be used
/// to send this task messages.
pub fn make_task<T>(socket: T) -> Handle
where
	T: grease::ServiceProvider<socket::Service> + Send + 'static,
{
	let (tx, rx) = mpsc::channel();
	let handle = Handle(tx.clone());
	std::thread::spawn(move || {
		let mut t = TaskContext::new(socket, handle);
		for msg in rx.iter() {
			t.handle(msg);
		}
		panic!("This task should never die!");
	});
	Handle(tx)
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

/// All our handler functions are methods on this TaskContext structure.
impl<S> TaskContext<S>
where
	S: grease::ServiceProvider<socket::Service>,
{
	/// Create a new TaskContext
	fn new(socket: S, us: Handle) -> TaskContext<S> {
		TaskContext {
			socket: socket,
			servers: MultiMap::new(),
			connections: MultiMap::new(),
			reply_to: us,
			// This number is arbitrary
			next_ctx: grease::Context::new(2_000),
			pending: HashMap::new(),
		}
	}

	/// Handle an incoming message. It might a `Request` for us,
	/// or it might be a `Confirm` or `Indication` from a lower layer.
	fn handle(&mut self, msg: Incoming) {
		match msg {
			// We only handle our own requests and responses
			Incoming::Request(x, reply_to) => {
				debug!("Rx: {:?}", x);
				self.handle_http_req(x, reply_to);
			}
			Incoming::SocketCfm(x) => {
				debug!("Rx: {:?}", x);
				self.handle_socket_cfm(x);
			}
			Incoming::SocketInd(x) => {
				debug!("Rx: {:?}", x);
				self.handle_socket_ind(x);
			}
			Incoming::Response(x) => {
				debug!("Rx: {:?}", x);
			}
		}
	}

	fn handle_socket_cfm(&mut self, cfm: socket::Confirm) {
		match cfm {
			socket::Confirm::Bind(x) => self.handle_socket_cfm_bind(x),
			socket::Confirm::Close(x) => self.handle_socket_cfm_close(x),
			socket::Confirm::Send(x) => self.handle_socket_cfm_send(x),
		}
	}

	fn handle_socket_ind(&mut self, ind: socket::Indication) {
		match ind {
			socket::Indication::Connected(x) => self.handle_socket_ind_connected(x),
			socket::Indication::Dropped(x) => self.handle_socket_ind_dropped(x),
			socket::Indication::Received(x) => self.handle_socket_ind_received(x),
		}
	}

	fn handle_http_req(&mut self, req: Request, reply_to: grease::ServiceUserHandle<Service>) {
		match req {
			Request::Bind(x) => self.handle_bind(x, reply_to),
			Request::ResponseStart(x) => self.handle_responsestart(x, reply_to),
			Request::ResponseBody(x) => self.handle_responsebody(x, reply_to),
		}
	}

	fn handle_bind(&mut self, req_bind: ReqBind, reply_to: grease::ServiceUserHandle<Service>) {
		let reply_ctx = ReplyContext {
			context: req_bind.context,
			reply_to: reply_to.clone(),
		};
		let server = Server {
			listen_handle: None,
			reply_ctx: Some(reply_ctx),
			our_handle: self.next_ctx.take(),
			ind_to: reply_to.clone(),
		};
		self.socket.send_request(
			socket::ReqBind {
				addr: req_bind.addr,
				context: server.our_handle,
				conn_type: socket::ConnectionType::Stream,
			}.into(),
			&self.reply_to,
		);
		self.servers.insert(server.our_handle, None, server);
	}

	/// Get the connection from a connection handle
	fn get_conn_by_http_handle(&mut self, handle: &ConnHandle) -> Option<&mut Connection> {
		self.connections.get_mut(handle)
	}

	/// Get the Server for a given socket ListenHandle.
	fn get_server_by_socket_handle(
		&mut self,
		handle: &socket::ListenHandle,
	) -> Option<&mut Server> {
		// The key here is Option<socket::ListenHandle> because
		// it takes time to bind the socket.
		self.servers.get_mut_alt(&Some(*handle))
	}

	/// Get the ServerHandle for a given socket ListenHandle. Used when
	/// we get a socket IndConnected.
	fn get_server_handle_by_socket_handle(
		&mut self,
		handle: &socket::ListenHandle,
	) -> Option<ServerHandle> {
		self.get_server_by_socket_handle(handle)
			.and_then(|x| Some(x.our_handle))
	}

	/// Get a reference to the appropriate Connection object, and its
	/// associated Server object. If one or the other isn't found, return
	/// None.
	fn get_conn_by_socket_handle(
		&mut self,
		handle: &socket::ConnHandle,
	) -> Option<(&mut Connection, &mut Server)> {
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
		headers: &HeaderMap,
	) -> String {
		let mut s = String::new();
		s.push_str(&format!("HTTP/1.1 {}\r\n", status));
		if !headers.contains_key("Server") {
			s.push_str("Server: grease/http\r\n");
		}
		if !headers.contains_key("Content-Length") {
			if let Some(l) = length {
				s.push_str(&format!("Content-Length: {}\r\n", l));
			}
		}
		if !headers.contains_key("Content-Type") {
			s.push_str(&format!("Content-Type: {}\r\n", content_type));
		}
		for (k, v) in headers.iter() {
			s.push_str(&format!("{}: {}\r\n", k.as_str(), v.to_str().unwrap()));
		}
		s.push_str("\r\n");
		return s;
	}

	/// @todo We should we check they call this once and only once.
	fn handle_responsestart(
		&mut self,
		req_start: ReqResponseStart,
		reply_to: grease::ServiceUserHandle<Service>,
	) {
		if self.get_conn_by_http_handle(&req_start.handle).is_some() {
			let skt = {
				let conn = self.get_conn_by_http_handle(&req_start.handle).unwrap();
				conn.body_length = req_start.length;
				conn.socket_handle
			};

			// Render the headers as a String
			// Send to the socket server
			// Send the cfm when the socket server has sent this data
			let s = Self::render_response(
				req_start.status,
				&req_start.content_type,
				req_start.length,
				&req_start.headers,
			);
			let req = socket::ReqSend {
				handle: skt,
				context: self.next_ctx.take(),
				data: s.into_bytes(),
			};
			let pend = PendingCfm {
				handle: req_start.handle,
				context: req_start.context,
				reply_to: reply_to.clone(),
				cfm_type: CfmType::Start,
				close_after: req_start.length == Some(0),
			};
			self.pending.insert(req.context, pend);
			self.socket.send_request(req.into(), &self.reply_to);
		} else {
			reply_to.send_confirm(
				CfmResponseStart {
					context: req_start.context,
					handle: req_start.handle,
					result: Err(Error::BadHandle),
				}.into(),
			);
		}
	}

	fn handle_responsebody(
		&mut self,
		req_body: ReqResponseBody,
		reply_to: grease::ServiceUserHandle<Service>,
	) {
		if self.get_conn_by_http_handle(&req_body.handle).is_some() {
			let mut close_after = false;
			let skt = {
				let conn = self.get_conn_by_http_handle(&req_body.handle).unwrap();
				match conn.body_length {
					Some(0) => {
						// This response has no body
						reply_to.send_confirm(
							CfmResponseBody {
								context: req_body.context,
								handle: req_body.handle,
								result: Err(Error::BadHandle),
							}.into(),
						);
						return;
					}
					Some(len) if req_body.data.len() > len => {
						// This body is too long
						reply_to.send_confirm(
							CfmResponseBody {
								context: req_body.context,
								handle: req_body.handle,
								result: Err(Error::BadHandle),
							}.into(),
						);
						return;
					}
					Some(len) if req_body.data.len() == len => {
						conn.body_length = Some(0);
						close_after = true;
						conn.socket_handle
					}
					Some(len) => {
						conn.body_length = Some(len - req_body.data.len());
						conn.socket_handle
					}
					None => conn.socket_handle,
				}
			};

			if req_body.data.len() > 0 {
				// Send to the socket server
				// Send the cfm when the socket server has sent this data
				let req = socket::ReqSend {
					handle: skt,
					context: self.next_ctx.take(),
					data: req_body.data.clone(),
				};
				let pend = PendingCfm {
					handle: req_body.handle,
					context: req_body.context,
					reply_to: reply_to.clone(),
					cfm_type: CfmType::Body,
					close_after: close_after,
				};
				self.pending.insert(req.context, pend);
				self.socket.send_request(req.into(), &self.reply_to);
			} else {
				// Close connection now!
				let req = socket::ReqClose {
					handle: skt,
					context: self.next_ctx.take(),
				};
				let pend = PendingCfm {
					handle: req_body.handle,
					context: req_body.context,
					reply_to: reply_to.clone(),
					cfm_type: CfmType::Body,
					close_after: false,
				};
				self.pending.insert(req.context, pend);
				self.socket.send_request(req.into(), &self.reply_to);
				let _ = self.connections.remove(&req_body.handle);
			}
		} else {
			reply_to.send_confirm(
				CfmResponseBody {
					context: req_body.context,
					handle: req_body.handle,
					result: Err(Error::BadHandle),
				}.into(),
			);
		}
	}

	fn send_response(&self, handle: &socket::ConnHandle, code: HttpResponseStatus, message: &str) {
		// An error occured which we must tell them about
		let mut r = rushttp::response::HttpResponse::new_with_body(code, "HTTP/1.0", message);
		r.add_header("Content-Type", "text/plain");
		let mut output = Vec::new();
		if let Ok(_) = r.write(&mut output) {
			self.socket.send_request(
				socket::ReqSend {
					handle: *handle,
					context: Context::default(),
					data: output,
				}.into(),
				&self.reply_to,
			);
		} else {
			warn!("Failed to render error");
		}

		self.socket.send_request(
			socket::ReqClose {
				handle: *handle,
				context: Context::default(),
			}.into(),
			&self.reply_to,
		);
	}

	fn map_result<T>(result: Result<T, socket::SocketError>) -> Result<(), Error> {
		match result {
			Ok(_) => Ok(()),
			Err(e) => Err(Error::SocketError(e)),
		}
	}

	/// Handle a response to a socket task bind request. It's either
	/// bound our socket, or it failed, but either way we must let
	/// our user know.
	fn handle_socket_cfm_bind(&mut self, cfm_bind: socket::CfmBind) {
		if let Some(mut server) = self.servers.remove(&cfm_bind.context) {
			let reply_ctx = server.reply_ctx.unwrap();
			server.reply_ctx = None;
			match cfm_bind.result {
				Ok(ref handle) => {
					server.listen_handle = Some(*handle);
					// Re-insert, but with socket handle as second key
					reply_ctx.reply_to.send_confirm(
						CfmBind {
							context: reply_ctx.context,
							result: Ok(server.our_handle),
						}.into(),
					);
					self.servers.insert(cfm_bind.context, Some(*handle), server);
				}
				Err(ref err) => {
					reply_ctx.reply_to.send_confirm(
						CfmBind {
							context: reply_ctx.context,
							result: Err(Error::SocketError(*err)),
						}.into(),
					);
				}
			}
		} else {
			warn!("Context {} not found", cfm_bind.context);
		}
	}

	fn handle_socket_cfm_close(&mut self, cfm: socket::CfmClose) {
		debug!("Got {:?}", cfm);
		if let Some(pend) = self.pending.remove(&cfm.context) {
			// Close out whatever triggered this close
			match pend.cfm_type {
				CfmType::Start => {
					pend.reply_to.send_confirm(
						CfmResponseStart {
							context: pend.context,
							handle: pend.handle,
							result: Self::map_result(cfm.result),
						}.into(),
					);
				}
				CfmType::Body => {
					pend.reply_to.send_confirm(
						CfmResponseBody {
							context: pend.context,
							handle: pend.handle,
							result: Self::map_result(cfm.result),
						}.into(),
					);
				}
				CfmType::Close => {
					// Nothing to send - internally generated
				}
			}
			let ind = IndClosed {
				handle: pend.handle,
			};
			pend.reply_to.send_indication(ind.into());
		}
	}

	fn handle_socket_cfm_send(&mut self, cfm: socket::CfmSend) {
		debug!("Got {:?}", cfm);
		if let Some(pend) = self.pending.remove(&cfm.context) {
			match pend.cfm_type {
				CfmType::Start => {
					pend.reply_to.send_confirm(
						CfmResponseStart {
							context: pend.context,
							handle: pend.handle,
							result: Self::map_result(cfm.result),
						}.into(),
					);
				}
				CfmType::Body => {
					pend.reply_to.send_confirm(
						CfmResponseBody {
							context: pend.context,
							handle: pend.handle,
							result: Self::map_result(cfm.result),
						}.into(),
					);
				}
				CfmType::Close => {
					// Should never happen
					panic!("Pend stored with CfmType::Close against ReqSend")
				}
			}
			if pend.close_after {
				// Close connection now!
				let req = socket::ReqClose {
					handle: cfm.handle,
					context: self.next_ctx.take(),
				};
				let pend = PendingCfm {
					handle: pend.handle,
					context: pend.context,
					reply_to: pend.reply_to,
					cfm_type: CfmType::Close,
					close_after: false,
				};
				let _ = self.connections.remove(&pend.handle);
				self.pending.insert(req.context, pend);
				self.socket.send_request(req.into(), &self.reply_to);
			}
		}
	}

	/// Someone has connected to our bound socket, so create a Connection
	/// and wait for data. We don't tell the layer above until we've parsed
	/// the HTTP Request header.
	fn handle_socket_ind_connected(&mut self, ind: socket::IndConnected) {
		debug!("Got {:?}", ind);
		if let Some(server_handle) = self.get_server_handle_by_socket_handle(&ind.listen_handle) {
			let conn = Connection {
				our_handle: self.next_ctx.take(),
				server_handle: server_handle,
				socket_handle: ind.conn_handle,
				parser: rushttp::request::Parser::new(),
				body_length: None,
			};
			debug!(
				"New connection {:?}, socket={:?}",
				conn.our_handle, conn.socket_handle
			);
			self.connections
				.insert(conn.our_handle, conn.socket_handle, conn);
		} else {
			warn!("Connection on non-existant socket handle");
		}
	}

	fn handle_socket_ind_dropped(&mut self, ind: socket::IndDropped) {
		self.connections.remove_alt(&ind.handle);
	}

	fn handle_socket_ind_received(&mut self, ind: socket::IndReceived) {
		debug!("Got {:?}", ind);
		let r = if let Some((conn, serv)) = self.get_conn_by_socket_handle(&ind.handle) {
			debug!("Got data for conn {:?}!", conn.our_handle);
			// Extract the fields from connection
			// As we can't keep a reference to it
			Some((
				conn.parser.parse(&ind.data),
				conn.our_handle,
				conn.server_handle,
				serv.ind_to.clone(),
			))
		} else {
			None
		};

		match r {
			Some((rushttp::request::ParseResult::Complete(req, _), ch, sh, ind_to)) => {
				// All done!
				ind_to.send_indication(
					IndRxRequest {
						server_handle: sh,
						connection_handle: ch,
						url: req.uri().clone(),
						method: req.method().clone(),
						headers: req.headers().clone(),
					}.into(),
				);
			}
			Some((rushttp::request::ParseResult::InProgress, _, _, _)) => {
				// Need more data
			}
			Some(_) => {
				self.delete_connection_by_socket_handle(&ind.handle);
				self.send_response(
					&ind.handle,
					rushttp::response::HttpResponseStatus::BadRequest,
					"Bad Request",
				);
			}
			None => {
				warn!("Data on non-existant socket handle");
			}
		}

		self.socket
			.send_response(socket::Response::Received(socket::RspReceived {
				handle: ind.handle,
			}));
	}
}

#[cfg(test)]
mod test {
	use std::net;
	use std::sync::mpsc;
	use std::sync::atomic;
	use super::*;

	use grease::ServiceProvider;

	enum TestIncoming {
		HttpCfm(Confirm),
		HttpInd(Indication),
		SocketReq(socket::Request, grease::ServiceUserHandle<socket::Service>),
		SocketRsp(socket::Response),
	}
	struct TestHandle(mpsc::Sender<TestIncoming>);

	static PORT_NUMBER: atomic::AtomicUsize = atomic::AtomicUsize::new(8000);

	const DEFAULT_TIMEOUT: ::std::time::Duration = ::std::time::Duration::from_secs(5);

	fn allocate_test_port() -> net::SocketAddr {
		let port = PORT_NUMBER.fetch_add(1, atomic::Ordering::SeqCst);
		net::SocketAddr::new(
			net::IpAddr::V4(net::Ipv4Addr::new(127, 0, 1, 1)),
			port as u16,
		)
	}

	impl grease::ServiceUser<Service> for TestHandle {
		fn send_confirm(&self, cfm: Confirm) {
			self.0.send(TestIncoming::HttpCfm(cfm)).unwrap();
		}
		fn send_indication(&self, ind: Indication) {
			self.0.send(TestIncoming::HttpInd(ind)).unwrap();
		}
		fn clone(&self) -> grease::ServiceUserHandle<Service> {
			Box::new(TestHandle(self.0.clone()))
		}
	}

	impl grease::ServiceProvider<socket::Service> for TestHandle {
		fn send_request(
			&self,
			req: socket::Request,
			reply_to: &grease::ServiceUser<socket::Service>,
		) {
			self.0
				.send(TestIncoming::SocketReq(req, reply_to.clone()))
				.unwrap();
		}
		fn send_response(&self, rsp: socket::Response) {
			self.0.send(TestIncoming::SocketRsp(rsp)).unwrap();
		}
	}

	fn make_test_channel() -> (TestHandle, mpsc::Receiver<TestIncoming>) {
		let (test_tx, rx) = mpsc::channel();
		(TestHandle(test_tx), rx)
	}

	fn bind_port<T>(
		this_thread: &TestHandle,
		test_rx: &mpsc::Receiver<TestIncoming>,
		http_north: &T,
		addr: &net::SocketAddr,
		ctx: Context,
		socket_handle: socket::ListenHandle,
	) -> (ServerHandle, grease::ServiceUserHandle<socket::Service>)
	where
		T: grease::ServiceProvider<Service>,
	{
		let bind_req = ReqBind {
			addr: addr.clone(),
			context: ctx,
		};
		http_north.send_request(bind_req.into(), this_thread);
		let cfm = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		let reply_to_copy: grease::ServiceUserHandle<socket::Service>;
		match cfm {
			TestIncoming::SocketReq(socket::Request::Bind(ref x), ref reply_to) => {
				assert_eq!(x.addr, *addr);
				let bind_cfm = socket::CfmBind {
					result: Ok(socket_handle),
					context: x.context,
				};
				reply_to.send_confirm(bind_cfm.into());
				reply_to_copy = (*reply_to).clone();
			}
			_ => panic!("Unexpected message."),
		}
		let cfm = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match cfm {
			TestIncoming::HttpCfm(Confirm::Bind(ref x)) => {
				assert_eq!(x.context, ctx);
				if let Ok(h) = x.result {
					(h, reply_to_copy.clone())
				} else {
					panic!("HTTP Bind failed");
				}
			}
			_ => panic!("Unexpected message"),
		}
	}

	#[test]
	fn bind_port_ok() {
		let (reply_to, test_rx) = make_test_channel();
		// Use ourselves as the 'socket' task
		let http_north = make_task(reply_to.clone());
		let _ = bind_port(
			&reply_to,
			&test_rx,
			&http_north,
			&allocate_test_port(),
			Context::new(1),
			Context::new(2),
		);
	}

	#[test]
	fn basic_get_vary_len() {
		let (reply_to, test_rx) = make_test_channel();
		// Use ourselves as the 'socket' task
		let http_north = make_task(reply_to.clone());
		let (sh, http_south) = bind_port(
			&reply_to,
			&test_rx,
			&http_north,
			&allocate_test_port(),
			Context::new(3),
			Context::new(4),
		);

		// ******************** Connection opens ********************

		let msg = socket::IndConnected {
			listen_handle: Context::new(4),
			conn_handle: Context::new(5),
			peer: "127.0.0.1:56789".parse().unwrap(),
		};
		http_south.send_indication(msg.into());

		// ******************** Request arrives ********************

		let msg = socket::IndReceived {
			handle: Context::new(5),
			data: String::from("GET /foo/bar HTTP/1.1\r\nHost: localhost\r\n\r\n").into_bytes(),
		};
		http_south.send_indication(msg.into());

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		let ch = match msg {
			TestIncoming::HttpInd(Indication::RxRequest(ref x)) => {
				assert_eq!(x.server_handle, sh);
				assert_eq!(x.url, "/foo/bar");
				assert_eq!(x.method, Method::GET);
				let mut expected_headers = HeaderMap::new();
				expected_headers.insert("HOST", "localhost".parse().unwrap());
				assert_eq!(x.headers, expected_headers);
				x.connection_handle
			}
			_ => panic!("Unexpected message"),
		};

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::SocketRsp(socket::Response::Received(ref x)) => {
				assert_eq!(x.handle, Context::new(5));
			}
			_ => panic!("Unexpected message"),
		};

		// ******************** Send the headers ********************

		let mut msg = ReqResponseStart {
			handle: ch,
			context: Context::new(1234),
			status: HttpResponseStatus::OK,
			content_type: String::from("text/plain"),
			length: None,
			headers: HeaderMap::new(),
		};
		msg.headers.insert("x-magic", "frobbins".parse().unwrap());
		http_north.send_request(msg.into(), &reply_to);

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::SocketReq(socket::Request::Send(ref x), ref msg_reply_to) => {
				assert_eq!(x.handle, Context::new(5));
				let headers = "HTTP/1.1 200 OK\r\nServer: grease/http\r\nContent-Type: \
				               text/plain\r\nx-magic: frobbins\r\n\r\n"
					.as_bytes();
				assert_eq!(x.data, headers);
				let send_cfm = socket::CfmSend {
					handle: x.handle,
					context: x.context,
					result: Ok(x.data.len()),
				};
				msg_reply_to.send_confirm(send_cfm.into());
			}
			_ => panic!("Unexpected message"),
		};

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::HttpCfm(Confirm::ResponseStart(ref x)) => {
				assert_eq!(x.handle, ch);
				assert_eq!(x.context, Context::new(1234));
				assert!(x.result.is_ok());
			}
			_ => panic!("Unexpected message"),
		};

		// ******************** Send the body ********************

		let test_body = Vec::from("One two, Rust on my shoe");
		let msg = ReqResponseBody {
			handle: ch,
			context: Context::new(5678),
			data: test_body.clone(),
		};
		http_north.send_request(msg.into(), &reply_to);

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::SocketReq(socket::Request::Send(ref x), ref msg_reply_to) => {
				assert_eq!(x.handle, Context::new(5));
				assert_eq!(x.data, test_body);
				let send_cfm = socket::CfmSend {
					handle: x.handle,
					context: x.context,
					result: Ok(x.data.len()),
				};
				msg_reply_to.send_confirm(send_cfm.into());
			}
			_ => panic!("Unexpected message"),
		};

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::HttpCfm(Confirm::ResponseBody(ref x)) => {
				assert_eq!(x.handle, ch);
				assert_eq!(x.context, Context::new(5678));
				assert!(x.result.is_ok());
			}
			_ => panic!("Unexpected message"),
		};

		// ******************** Send an empty body ********************

		let msg = ReqResponseBody {
			handle: ch,
			context: Context::new(5678),
			data: Vec::new(),
		};
		http_north.send_request(msg.into(), &reply_to);

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::SocketReq(socket::Request::Close(ref x), ref msg_reply_to) => {
				assert_eq!(x.handle, Context::new(5));
				let send_cfm = socket::CfmClose {
					handle: x.handle,
					context: x.context,
					result: Ok(()),
				};
				msg_reply_to.send_confirm(send_cfm.into());
			}
			_ => panic!("Unexpected message"),
		};

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::HttpCfm(Confirm::ResponseBody(ref x)) => {
				assert_eq!(x.handle, ch);
				assert_eq!(x.context, Context::new(5678));
				assert!(x.result.is_ok());
			}
			_ => panic!("Unexpected message"),
		};

		// ******************** Get a close indication ********************

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::HttpInd(Indication::Closed(ref x)) => {
				assert_eq!(x.handle, ch);
			}
			_ => panic!("Unexpected message"),
		};

		// ******************** All done ********************
	}

	#[test]
	fn basic_get_fixed_len() {
		let (reply_to, test_rx) = make_test_channel();
		// Use ourselves as the 'socket' task
		let http_north = make_task(reply_to.clone());
		let (sh, http_south) = bind_port(
			&reply_to,
			&test_rx,
			&http_north,
			&allocate_test_port(),
			Context::new(3),
			Context::new(4),
		);

		// ******************** Connection opens ********************

		let msg = socket::IndConnected {
			listen_handle: Context::new(4),
			conn_handle: Context::new(5),
			peer: "127.0.0.1:56789".parse().unwrap(),
		};
		http_south.send_indication(msg.into());

		// ******************** Request arrives ********************

		let msg = socket::IndReceived {
			handle: Context::new(5),
			data: String::from("GET /foo/bar HTTP/1.1\r\nHost: localhost\r\n\r\n").into_bytes(),
		};
		http_south.send_indication(msg.into());

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		let ch = match msg {
			TestIncoming::HttpInd(Indication::RxRequest(ref x)) => {
				assert_eq!(x.server_handle, sh);
				assert_eq!(x.url, "/foo/bar");
				assert_eq!(x.method, Method::GET);
				let mut expected_headers = HeaderMap::new();
				expected_headers.insert("Host", "localhost".parse().unwrap());
				assert_eq!(x.headers, expected_headers);
				x.connection_handle
			}
			_ => panic!("Unexpected message"),
		};

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::SocketRsp(socket::Response::Received(ref x)) => {
				assert_eq!(x.handle, Context::new(5));
			}
			_ => panic!("Unexpected message"),
		};

		// ******************** Send the headers ********************

		let mut msg = ReqResponseStart {
			handle: ch,
			context: Context::new(1234),
			status: HttpResponseStatus::OK,
			content_type: String::from("text/plain"),
			length: Some(24),
			headers: HeaderMap::new(),
		};
		msg.headers.insert("x-magic", "frobbins".parse().unwrap());
		http_north.send_request(msg.into(), &reply_to);

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::SocketReq(socket::Request::Send(ref x), ref msg_reply_to) => {
				assert_eq!(x.handle, Context::new(5));
				let headers = "HTTP/1.1 200 OK\r\nServer: grease/http\r\nContent-Length: \
				               24\r\nContent-Type: text/plain\r\nx-magic: frobbins\r\n\r\n"
					.as_bytes();
				println!("Headers: {:?}", String::from_utf8(x.data.clone()));
				assert_eq!(x.data, headers);
				let send_cfm = socket::CfmSend {
					handle: x.handle,
					context: x.context,
					result: Ok(x.data.len()),
				};
				msg_reply_to.send_confirm(send_cfm.into());
			}
			_ => panic!("Unexpected message"),
		};

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::HttpCfm(Confirm::ResponseStart(ref x)) => {
				assert_eq!(x.handle, ch);
				assert_eq!(x.context, Context::new(1234));
				assert!(x.result.is_ok());
			}
			_ => panic!("Unexpected message"),
		};

		// ******************** Send the body ********************

		let test_body = Vec::from("One two, Rust on my shoe");
		let msg = ReqResponseBody {
			handle: ch,
			context: Context::new(5678),
			data: test_body.clone(),
		};
		http_north.send_request(msg.into(), &reply_to);

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::SocketReq(socket::Request::Send(ref x), ref msg_reply_to) => {
				assert_eq!(x.handle, Context::new(5));
				assert_eq!(x.data, test_body);
				let send_cfm = socket::CfmSend {
					handle: x.handle,
					context: x.context,
					result: Ok(x.data.len()),
				};
				msg_reply_to.send_confirm(send_cfm.into());
			}
			_ => panic!("Unexpected message"),
		};

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::HttpCfm(Confirm::ResponseBody(ref x)) => {
				assert_eq!(x.handle, ch);
				assert_eq!(x.context, Context::new(5678));
				assert!(x.result.is_ok());
			}
			_ => panic!("Unexpected message"),
		};

		// ******************** Auto close ********************

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::SocketReq(socket::Request::Close(ref x), ref msg_reply_to) => {
				assert_eq!(x.handle, Context::new(5));
				let send_cfm = socket::CfmClose {
					handle: x.handle,
					context: x.context,
					result: Ok(()),
				};
				msg_reply_to.send_confirm(send_cfm.into());
			}
			_ => panic!("Unexpected message"),
		};

		// ******************** Get a close indication ********************

		let msg = test_rx.recv_timeout(DEFAULT_TIMEOUT).unwrap();
		match msg {
			TestIncoming::HttpInd(Indication::Closed(ref x)) => {
				assert_eq!(x.handle, ch);
			}
			_ => panic!("Unexpected message"),
		};

		// ******************** All done ********************
	}
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
