//! # WebServ
//! This is a really basic REST server, which uses the `http` task to handle
//! the HTTP protocol. It can handle OPTION requests which describe the set
//! of URLs supported, but when a POST/GET/PUT/DELETE is received, it informs
//! the layer above and waits for that layer to Request some data to be sent
//! in reply.
//!
//! The language is a little tricky because an HTTP Request is represented
//! by a `webserv::Indication` going up the stack, and then an upper layer
//! sends back a `webserv::Request` to request that webserv transmit a
//! HTTP response in reply to the HTTP request.
//!
//! The URL map for this example is:
//!
//! |URL        | Methods         | Message                |
//! |-----------|-----------------|------------------------|
//! | /status   | GET             | IndStatusGetReceived   |
//! |           |                 | ReqSendStatusGetResult |
//!
//! More URLs will follow.

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use http;
use http::User;

// ****************************************************************************
//
// Public Messages
//
// ****************************************************************************

/// Requests that can be sent to the webserv task.
#[derive(Debug)]
pub enum Request {
	SendStatusGetResult(Box<ReqSendStatusGetResult>),
}

/// Confirmations sent in reply to Requests.
#[derive(Debug)]
pub enum Confirmation {
	SendStatusGetResult(Box<CfmSendStatusGetResult>),
}

/// Indications that can be sent from the webserv task.
#[derive(Debug)]
pub enum Indication {
	StatusGetReceived(Box<IndStatusGetReceived>),
}

/// Instructs this task to send a reply to an earlier
/// GET on /status.
#[derive(Debug)]
pub struct ReqSendStatusGetResult {
	pub context: WebRequestHandle,
	pub status: u32,
}

/// Informs the layer above that the ReqSendStatusGetResult
/// has been processed and the connection is closed.
#[derive(Debug)]
pub struct CfmSendStatusGetResult {
	pub context: WebRequestHandle,
	pub result: Result<(), Error>,
}

/// Indicates a GET has been performed on /status
#[derive(Debug)]
pub struct IndStatusGetReceived {
	pub context: WebRequestHandle,
}

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Uniquely identifies an HTTP request received from an HTTP client.
/// Used when sending a response to the HTTP request.
pub type WebRequestHandle = ::Context;

/// All possible webserv task errors
#[derive(Debug, Copy, Clone)]
pub enum Error {
	/// Used when I'm writing code and haven't added the correct error yet
	Unknown,
	/// Used if a ReqXXXResult is sent on an invalid (perhaps recently
	/// closed) WebRequestHandle.
	BadHandle,
	/// http task failed,
	HttpError(http::Error),
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

struct TaskContext {
	/// Who we send http messages to
	http: ::MessageSender,
	/// How other tasks send messages to us
	reply_to: ::MessageSender,
}

// ****************************************************************************
//
// Public Functions
//
// ****************************************************************************

/// Creates a new socket task. Returns an object that can be used
/// to send this task messages.
pub fn make_task(http: &::MessageSender) -> ::MessageSender {
	let local_http = http.clone();
	::make_task("webserv",
	            move |rx: ::MessageReceiver, tx: ::MessageSender| main_loop(rx, tx, local_http))
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

/// The task runs this main loop indefinitely.
fn main_loop(rx: ::MessageReceiver, tx: ::MessageSender, http: ::MessageSender) {
	let mut t = TaskContext::new(http, tx);
	t.init();
	for msg in rx.iter() {
		t.handle(msg);
	}
	panic!("This task should never die!");
}

/// All our handler functions are methods on this TaskContext structure.
impl TaskContext {
	/// Create a new TaskContext
	fn new(http: ::MessageSender, us: ::MessageSender) -> TaskContext {
		TaskContext {
			http: http,
			reply_to: us,
		}
	}

	/// Register an HTTP server
	fn init(&mut self) {
		let msg = http::ReqBind {
			addr: "0.0.0.0:8000".parse().unwrap(),
			context: 0,
		};
		self.http.send_request(msg, &self.reply_to);
	}

	/// Handle an incoming message. It might a `Request` for us,
	/// or it might be a `Confirmation` or `Indication` from a lower layer.
	fn handle(&mut self, msg: ::Message) {
		match msg {
			// We only handle our own requests and responses
			::Message::Request(ref reply_to, ::Request::WebServ(ref x)) => {
				self.handle_webserv_req(x, reply_to)
			}
			// We use the http task so we expect to get confirmations and indications from it
			::Message::Confirmation(::Confirmation::Http(ref x)) => self.handle_http_cfm(x),
			::Message::Indication(::Indication::Http(ref x)) => self.handle_http_ind(x),
			// If we get here, someone else has made a mistake
			_ => error!("Unexpected message in webserv task: {:?}", msg),
		}
	}

	// Handle our own requests.
	fn handle_webserv_req(&mut self, req: &Request, reply_to: &::MessageSender) {
		match *req {
			Request::SendStatusGetResult(ref x) => {
				self.handle_webserv_sendstatusgetresult_req(x, reply_to)
			}
		}
	}

	fn handle_webserv_sendstatusgetresult_req(&mut self,
	                                          _msg: &ReqSendStatusGetResult,
	                                          _reply_to: &::MessageSender) {

	}
}

impl http::User for TaskContext {
	/// Called when a Bind confirmation is received.
	fn handle_http_cfm_bind(&mut self, _msg: &http::CfmBind) {}

	/// Called when an ResponseStart confirmation is received.
	fn handle_http_cfm_response_start(&mut self, _msg: &http::CfmResponseStart) {}

	/// Called when a ResponseBody confirmation is received.
	fn handle_http_cfm_response_body(&mut self, _msg: &http::CfmResponseBody) {}

	/// Handles a Connected indication.
	fn handle_http_ind_connected(&mut self, _msg: &http::IndConnected) {}

	/// Handles a connection Closed indication.
	fn handle_http_ind_closed(&mut self, _msg: &http::IndClosed) {}
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
