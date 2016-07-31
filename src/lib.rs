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
//!
//! ## Usage
//!
//! To use cuslip, make sure your program/crate has:
//!
//! ```
//! use cuslip::prelude::*;
//! use cuslip;
//! ```

#![allow(dead_code)]

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

#[macro_use]
extern crate log;
extern crate mio;
extern crate rushttp;

pub mod socket;
pub mod prelude;
pub mod http;

use ::prelude::*;
use std::sync::mpsc;
use std::thread;

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// We use this for sending messages into a task
#[derive(Debug)]
pub struct MessageSender(mpsc::Sender<Message>);

/// A task uses this internally for pending on received messages
#[derive(Debug)]
pub struct MessageReceiver(mpsc::Receiver<Message>);

/// A type used to passing context between layers. If each layer maintains
/// a HashMap<Context, T>, when a confirmation comes back from the layer
/// below, it's easy to work out which T it corresponds to.
pub type Context = usize;

/// A message is the fundametal unit we pass between tasks.
/// All messages have a body, but requests also have an mpsc Channel
/// object that the matching confirmation should be sent to.
#[derive(Debug)]
pub enum Message {
    Request(MessageSender, Request),
    Confirmation(Confirmation),
    Indication(Indication),
    Response(Response),
}

/// The set of all requests in the system. This is an enumeration of all the
/// services that can handle requests. The enum included within each service is probably
/// defined in that service's module.
#[derive(Debug)]
pub enum Request {
    Generic(GenericReq),
    Socket(socket::SocketReq),
    Http(http::HttpReq),
}

/// The set of all confirmations in the system. This should look exactly like
/// `Request` but with Cfm instead of Req. These are handled by tasks that
/// send requests - you send a request and you get a confirm back.
#[derive(Debug)]
pub enum Confirmation {
    Generic(GenericCfm),
    Socket(socket::SocketCfm),
    Http(http::HttpCfm),
}

/// The set of all indications in the system. This is an enumeration of all the
/// services that can send indications. The enum included within each
/// service is probably defined in that service's module.
#[derive(Debug)]
pub enum Indication {
    Socket(socket::SocketInd),
    Http(http::HttpInd),
}

/// The set of all responses in the system. This is an enumeration of all the
/// services that need responses (which is actually quite rare). The enum
/// included within each service is probably defined in that service's module.
#[derive(Debug)]
pub enum Response {
    Socket(socket::SocketRsp),
}

/// Generic requests should be handled by every task.
#[derive(Debug)]
pub enum GenericReq {
    Ping(Box<PingReq>),
}

/// There is exactly one GenericCfm for every GenericReq. These should be
/// handled by every task that can ever send a GenericReq.
#[derive(Debug)]
pub enum GenericCfm {
    Ping(Box<PingCfm>),
}

/// A simple ping - generates a PingCfm with some reflected context.
#[derive(Debug)]
pub struct PingReq {
    context: Context,
}

/// Reply to a PingReq, including the reflected context.
#[derive(Debug)]
pub struct PingCfm {
    context: Context,
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
/// fn main_loop(rx: cuslip::MessageReceiver, _: cuslip::MessageSender) {
///     loop {
///         let _ = rx.recv().unwrap();
///     }
/// }
/// # fn main() {
/// cuslip::make_task("foo", main_loop);
/// # }
/// ```
pub fn make_task<F>(name: &str, main_loop: F) -> MessageSender
    where F: FnOnce(MessageReceiver, MessageSender),
          F: Send + 'static
{
    let (sender, receiver) = make_channel();
    let angle_name = format!("<{}>", name);
    let sender_clone = sender.clone();
    let _ =
        thread::Builder::new().name(angle_name).spawn(move || main_loop(receiver, sender_clone));
    return sender;
}

/// Helper function to create an mpsc channel pair.
pub fn make_channel() -> (MessageSender, MessageReceiver) {
    let (tx, rx) = mpsc::channel::<Message>();
    return (MessageSender(tx), MessageReceiver(rx));
}

impl Drop for Message {
    fn drop(&mut self) {
        debug!("** Destroyed {:?}", self);
    }
}

impl MessageSender {
    pub fn send_request<T: RequestSendable>(&self, msg: T, reply_to: &MessageSender) {
        self.0.send(msg.wrap(reply_to)).unwrap();
    }

    pub fn send_message<T: NonRequestSendable>(&self, msg: T) {
        self.0.send(msg.wrap()).unwrap();
    }

    pub fn send(&self, msg: Message) {
        self.0.send(msg).unwrap()
    }

    pub fn clone(&self) -> MessageSender {
        MessageSender(self.0.clone())
    }
}

impl MessageReceiver {
    pub fn recv(&self) -> Result<Message, std::sync::mpsc::RecvError> {
        self.0.recv()
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
    #[test]
    fn test_make_channel() {
        let (tx, rx) = ::make_channel();
        let test_req = ::PingReq { context: 1234 };
        tx.send_request(test_req, &tx);
        let msg = rx.recv();
        println!("Got {:?}", msg);
        let msg = msg.unwrap();
        match msg {
            ::Message::Request(_, ::Request::Generic(::GenericReq::Ping(ref x))) => {
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
