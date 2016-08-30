//! # WebServ
//! This is a really basic REST server, which uses the `http` task to handle
//! the HTTP protocol. It can handle OPTION requests which describe the set
//! of URLs supported, but when a POST/GET/PUT/DELETE is received, it informs
//! the layer above and waits for that layer to Request some data to be sent
//! in reply.
//!
//! The URL map for this example is:
//!
//! |URL        | Methods         |
//! |-----------|-----------------|
//! | /status   | GET             |
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

type WebRequestHandle = ::Context;

/// Requests that can be sent to the webserv task.
#[derive(Debug)]
pub enum Request {
    SendStatusGetResult(Box<ReqSendStatusGetResult>)
}

/// Confirmations sent in reply to Requests.
#[derive(Debug)]
pub enum Confirmation {
    SendStatusGetResult(Box<CfmSendStatusGetResult>)
}

/// Indications that can be sent from the webserv task.
pub enum Indication {
    StatusGet(Box<IndStatusGet>)
}

/// Instructs this task to send a reply to an earlier
/// GET on /status.
pub struct ReqSendStatusGetResult {
    pub context: WebRequestHandle,
    pub status: u32
}

/// Informs the layer above that the ReqSendStatusGetResult
/// has been processed and the connection is closed.
pub struct CfmSendStatusGetResult {
    pub context: WebRequestHandle,
    pub result: Result<(), Error>
}

/// Indicates a GET has been performed on /status
pub struct IndStatusGet {
    pub context: WebRequestHandle
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
