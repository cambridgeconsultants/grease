//! # cuslip - Making threads easier when you have rust.
//!
//! ## Overview
//!
//! For an high level overview to cuslip, see the project's README.md file.
//!
//! cuslip is a message-passing system, and messages are passed between tasks.
//! Each task should be in its own module, and it should implement a
//! `make_thread()` function, like this:
//!
//! ```
//! pub fn make_thread() -> cuslip::MessageSender {
//!     let (sender, receiver) = cuslip::make_channel();
//!     thread::spawn(move || main_loop(receiver));
//!     return sender;
//! }
//! ```
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
//! Here's an example of what might be a in Foo module:
//!
//! ```
//! #[derive(Debug)]
//! pub enum FooReq {
//!     /// Open a foo
//!     Open(Box<FooReqOpen>),
//!     /// Close an open foo
//!     Close(Box<FooReqClose>),
//! }
//!
//! #[derive(Debug)]
//! pub struct FooReqOpen {
//!     pub bar: String,
//!     pub baz: u32,
//! }
//!
//! impl RequestSendable for FooReqOpen {
//!     fn wrap(self, reply_to: &MessageSender) -> Message {
//!         Message::Request(reply_to.clone(), Request::Foo(FooReq::Open(Box::new(self))))
//!     }
//! }
//! ```
//!
//! Here's what you add to cuslip to register your task.
//!
//! ```
//! use foo;
//! pub enum Request {
//!     Foo(foo::FooReq),
//! }
//! ```
//!
//! Finally, here's an (incomplete) module that uses Foo:
//!
//! ```
//! struct FooUser {
//!     foo_handle: MessageSender,
//!     tx: MessageSender,
//!     rx: MessageReceiver,
//! }
//!
//! impl FooUser {
//!     fn main_loop(&mut self) {
//!         loop {
//!             let msg = self.rx.recv().unwrap();
//!             match msg {
//!                 /// Do something in `handle_req`, and send the confirmation to `reply_to`
//!                 Message::Request(reply_to, Request::Foo(x)) => self.handle_req(reply_to, x)
//!             }
//!         }
//!     }
//!
//!     fn test_fn(&self) {
//!         // assume we have a handle to a foo task we can
//!         // send Foo requests on, and we have
//!         let msg = FooReqOpen {
//!             bar: "test".to_owned(),
//!             baz: 1234
//!         };
//!         self.foo_handle.send(msg.wrap(self.tx.clone()).unwrap()
//!     }
//! }
//! ```

#![allow(dead_code)]

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use std::sync::mpsc;
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
}

/// The set of all requests in the system.
/// This is an enumeration of all the interfaces.
#[derive(Debug)]
pub enum Request {
    Socket(socket::SocketReq),
}

/// The set of all confirmations in the system.
/// This is an enumeration of all the interfaces.
#[derive(Debug)]
pub enum Confirmation {
    Socket(socket::SocketCfm),
}

/// The set of all indications in the system.
/// This is an enumeration of all the interfaces.
#[derive(Debug)]
pub enum Indication {
    Socket(socket::SocketInd),
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

/// Helper function to create an mpsc channel pair.
pub fn make_channel() -> (MessageSender, MessageReceiver) {
    let (tx, rx) = mpsc::channel::<Message>();
    return (tx, rx);
}

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
