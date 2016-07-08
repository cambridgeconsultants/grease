//! # cuslip - Making threads easier when you have rust.
//!
//! ## Overview
//!
//! For an high level overview to cuslip, see the project's README.md file.
//!
//! cuslip is a message-passing system, and messages are passed between tasks.
//! Each task should be in its own module, and it should implement some sort
//! of init function which calls `cuslip::make_task`.
//!
//! `main_loop` is a function which calls `recv()` on the given receiver object and
//! performs the appropriate action when a message is received.
//!
//! ## Messages
//!
//! Messages in cuslip are boxed Structs, wrapped inside a nested enum which
//! identifies which Struct they are. This allows them to be passed through a
//! `std::sync::mpsc::Channel`. The Box ensures the messages are all small,
//! as opposed to all being the size of the largest message.
//!
//! The wrapping is handled semi-automatically. I might macro this in future.
//!
//! See the `socket` module for an example,

#![allow(dead_code)]

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

#[macro_use]
extern crate log;
extern crate mio;

use std::sync::mpsc;
use std::thread;

pub mod socket;

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// We use this for sending messages into a task
pub type MessageSender = mpsc::Sender<Message>;

/// A task uses this internally for pending on received messages
pub type MessageReceiver = mpsc::Receiver<Message>;

/// A message is the fundametal unit we pass between tasks.
/// All messages have a body, but requests also have an mpsc Channel
/// object that the match confirmation should be sent to.
#[derive(Debug)]
pub enum Message {
    Request(MessageSender, Request),
    Confirmation(Confirmation),
    Indication(Indication),
    Response(Response),
}

/// The set of all requests in the system.
/// This is an enumeration of all the interfaces.
#[derive(Debug)]
pub enum Request {
    Generic(GenericReq),
    Socket(socket::SocketReq),
}

/// The set of all confirmations in the system.
/// This is an enumeration of all the interfaces.
#[derive(Debug)]
pub enum Confirmation {
    Generic(GenericCfm),
    Socket(socket::SocketCfm),
}

/// The set of all indications in the system.
/// This is an enumeration of all the interfaces.
#[derive(Debug)]
pub enum Indication {
    Socket(socket::SocketInd),
}

/// The set of all responses in the system.
/// This is an enumeration of all the interfaces.
#[derive(Debug)]
pub enum Response {
    Socket(socket::SocketRsp),
}

/// Implementors of the NonRequestSendable trait can be easily wrapped in a message
/// ready for sending down a MessageSender channel endpoint. All Indication, Confirmation
/// and Response messages must implement this.
pub trait NonRequestSendable {
    fn wrap(self) -> Message;
}

/// Implementors of the RequestSendable trait can be easily wrapped in a
/// message ready for sending down a MessageSender channel endpoint. All
/// Request messages must implement this.
pub trait RequestSendable {
    fn wrap(self, reply_to: &MessageSender) -> Message;
}

/// Generic requests should be handled by every task
#[derive(Debug)]
pub enum GenericReq {
    Ping(Box<PingReq>),
}

/// There is exactly one GenericCfm for every GenericReq
#[derive(Debug)]
pub enum GenericCfm {
    Ping(Box<PingCfm>),
}

/// A simple ping - generates a PingCfm
#[derive(Debug)]
pub struct PingReq {
    context: usize,
}

/// Reply to a PingReq
#[derive(Debug)]
pub struct PingCfm {
    context: usize,
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

// None

// ****************************************************************************
//
// Public Functions
//
// ****************************************************************************

/// Helper function to create a new thread.
///
/// ```
/// fn main_loop(rx: cuslip::MessageReceiver) {
///     loop {
///         let _ = rx.recv().unwrap();
///     }
/// }
/// # fn main() {
/// cuslip::make_task("foo", main_loop);
/// # }
/// ```
pub fn make_task<F>(name: &str, main_loop: F) -> MessageSender
    where F: FnOnce(MessageReceiver),
          F: Send + 'static
{
    let (sender, receiver) = make_channel();
    let angle_name = format!("<{}>", name);
    let _ = thread::Builder::new().name(angle_name).spawn(move || main_loop(receiver));
    return sender;
}

/// Helper function to create an mpsc channel pair.
pub fn make_channel() -> (MessageSender, MessageReceiver) {
    let (tx, rx) = mpsc::channel::<Message>();
    return (tx, rx);
}

impl Drop for Message {
    fn drop(&mut self) {
        debug!("** Destroyed {:?}", self);
    }
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

/// PingReq is sendable over a channel
impl RequestSendable for PingReq {
    fn wrap(self, reply_to: &MessageSender) -> Message {
        Message::Request(reply_to.clone(),
                         Request::Generic(GenericReq::Ping(Box::new(self))))
    }
}

/// PingCfm is sendable over a channel
impl NonRequestSendable for PingCfm {
    fn wrap(self) -> Message {
        Message::Confirmation(Confirmation::Generic(GenericCfm::Ping(Box::new(self))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_channel() {
        let (tx, rx) = super::make_channel();
        let test_req = super::PingReq { context: 1234 };
        tx.send(test_req.wrap(&tx)).unwrap();
        let msg = rx.recv();
        println!("Got {:?}", msg);
        let msg = msg.unwrap();
        match msg {
            Message::Request(_, Request::Generic(GenericReq::Ping(ref x))) => {
                assert_eq!(x.context, 1234);
            }
            _ => panic!("Bad match"),
        }
    }
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
