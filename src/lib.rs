//! # grease - Making threads easier when you have rust.
//!
//! ## Overview
//!
//! For an high level overview to grease, see the project's README.md file.
//!
//! grease is a message-passing system, and messages are passed between tasks.
//! Tasks receive messages and act on them - be that, sending an immediate reply
//! or communicating with a task lower down (or some other subsystem) before
//! replying. Typically, tasks will implement a Finite State Machine (FSM) to
//! control their actions.
//!
//! Each task should be in its own module, and it should implement some sort
//! of init function which calls `grease::make_task`, passing in a function
//! which forms the tasks main loop.
//!
//! This main loop should repeatedly call the blocking `recv()` method on the
//! given receiver object and perform the appropriate action when a message
//! is received.
//!
//! ## Messages
//!
//! Messages in grease are boxed Structs, wrapped inside a nested enum which
//! identifies which Struct they are. This allows them to be passed through a
//! `std::sync::mpsc::Channel`, which is the message passing system underneath
//! grease. The Box ensures the messages are all small, as opposed to all
//! being the size of the largest message. The `Channel` is wrapped in a
//! `MessageSender` object, which you keep, and pass around copies created
//! with `clone()`.
//!
//! If task A uses task B, then task A's init function should receive a copy
//! of task B's `MessageSender` object. When task A sends a message to task B,
//! task A puts its own `MessageSender` in the Request, so that task B knows
//! where to send the Confirmation to. In some cases, it is appropriate for task B
//! to retain a copy of the `MessageSender` so that indications can be sent at
//! a later date. It is not recommended for task B to receive task A's `MessageSender`
//! other than in a Request (for example, do not pass it in an init function), to
//! avoid un-necessary coupling.
//!
//! See the `http` module for an example - it uses the `socket` module.
//!
//! ## Usage
//!
//! To use grease, make sure your program/crate has:
//!
//! ```
//! use grease::prelude::*;
//! use grease;
//! ```
//!
//! You will also need to modify the `Message` enum (and its nested enums within)
//! to encompass any modules you have written. They cannot currently be registered
//! dynamically.

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
extern crate multi_map;

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

/// When handling a request, the process may take some time. As the request
/// must be destroyed as soon as it arrives (for logging purposes), the
/// essential details are recorded so that a Confirmation can be sent at a
/// later date.
pub struct ReplyContext {
    pub reply_to: ::MessageSender,
    pub context: ::Context,
}

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
/// fn main_loop(rx: grease::MessageReceiver, _: grease::MessageSender) {
///     loop {
///         let _ = rx.recv().unwrap();
///     }
/// }
/// # fn main() {
/// grease::make_task("foo", main_loop);
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

/// This dumps messages to the logger when they are dropped (i.e. once they have been handled).
/// This means the log should be entirely sufficient to determine what the system has done, which
/// is invaluable for debugging purposes.
///
/// In a future version, this logging might be in binary format over some sort of socket.
impl Drop for Message {
    fn drop(&mut self) {
        debug!("** Destroyed {:?}", self);
    }
}

/// This wraps up an mpsc::Sender, performing a bit of repetitive code
/// required to Box up `RequestSendable` and `NonRequestSendable` messages and
/// wrap them in a covering `Message` enum.
impl MessageSender {
    /// Used for sending requests to a task. The `reply_to` value is a separate argument
    /// because it is mandatory. It would be an error to send a request without indicating
    /// where the matching confirmation should be sent.
    pub fn send_request<T: RequestSendable>(&self, msg: T, reply_to: &MessageSender) {
        self.0.send(msg.wrap(reply_to)).unwrap();
    }

    /// Used for sending confirmations, indications and responses to a task.
    /// There is no `reply_to` argument because these messages do not usually
    /// ellicit a response. The exception is that some Indications do ellicit
    /// a Response, but where this is the case, the receiving task will
    /// already know where the `Indication` came from as it must have first sent
    /// a `Request` to trigger the `Indication` in the first place.
    pub fn send_nonrequest<T: NonRequestSendable>(&self, msg: T) {
        self.0.send(msg.wrap()).unwrap();
    }

    /// Used for sending wrapped `Message` objects. Not often required - use
    /// `send_request` and `send_nonrequest` in preference.
    pub fn send(&self, msg: Message) {
        self.0.send(msg).unwrap()
    }

    /// Creates a clone of this MessageSender. The clone can then be kept
    /// for sending messages at a later date, without consuming the original.
    pub fn clone(&self) -> MessageSender {
        MessageSender(self.0.clone())
    }
}

/// This wraps up an mpsc::Sender, performing a bit of repetitive boilerplate code.
impl MessageReceiver {
    /// Useful for test code, but a task implementation should call `iter` in preference.
    pub fn recv(&self) -> Result<Message, mpsc::RecvError> {
        self.0.recv()
    }

    /// Allows the caller to repeatedly block on new messages.
    /// Iteration ends when channel is destroyed (usually on system shutdown).
    pub fn iter(&self) -> mpsc::Iter<Message> {
        self.0.iter()
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
