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

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Uniquely identifies an HTTP request received from an HTTP client.
/// Used when sending a response to the HTTP request.
pub type WebRequestHandle = ::Context;

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

// None

// ****************************************************************************
//
// Private Data
//
// ****************************************************************************

// None

// ****************************************************************************
//
// Public Functions
//
// ****************************************************************************

// None

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

// None

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
