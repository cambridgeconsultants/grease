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
//! which forms the task's main loop.
//!
//! ## Messages
//!
//! Messages in grease are boxed structs, wrapped inside a nested enum which
//! identifies which struct they are. This allows them to all be passed through
//! a single `std::sync::mpsc::Channel`, which (wrapped in a `MessageSender`) is
//! the message passing system underneath grease. The Box ensures the messages
//! are all small, as opposed to all being the size of the largest message.
//!
//! If task A uses task B, then task A's init function should receive a copy of
//! task B's `MessageSender` object. When task A sends a message to task B, task
//! A puts its own `MessageSender` in the Request, so that task B knows where to
//! send the Confirmation to. In some cases, it is appropriate for task B to
//! retain a copy of the `MessageSender` so that indications can be sent at a
//! later date. It is not recommended for task B to receive task A's
//! `MessageSender` other than in a Request (for example, do not pass it in an
//! init function), to avoid un-necessary coupling.
//!
//! See the `http` module for an example - it uses the `socket` module.
//!
//! ## Usage
//!
//! To use grease, make sure your program/crate has:
//!
//! ```ignore
//! use grease::prelude::*;
//! use grease;
//! ```
//!
//! You will also need to modify the `Message` enum (and its nested enums
//! within) to encompass any modules you have written. They cannot currently be
//! registered dynamically.
//!
//! ## Implementing a task
//!
//! The task's main loop function is given a `MessageReceiver`, and a
//! `MessageSender`. The task should loop on the `MessageReceiver` object's
//! ``iter()` method and perform the appropriate action when a message is
//! received. The task should end when the iterator terminates. The
//! `MessageSender` is probably not useful within the task itself (unless it
//! wishes to send itself a message) but clones are usually passed to other
//! tasks, so they know how to respond to this task.
//!
//! ```ignore
//! use grease;
//! ...
//! grease::make_task("foo", main_loop);
//! ...
//! fn main_loop(rx: grease::MessageReceiver, tx: grease::MessageSender) {
//!     let t = TaskContext::new(tx);
//!     for msg in rx.iter() {
//!         t.handle(msg);
//!     }
//! }
//! ```

// ****************************************************************************
//
// Macros
//
// ****************************************************************************

/// Implements RequestSendable on the given request structure.
/// For example:
///
/// ```ignore
/// mod foo {
///     make_request(ReqOpen, grease::Request::Foo, Request::Open)
/// }
/// ```
///
/// Where `ReqOpen` is the name of a struct, `grease::Request::Foo` is the
/// member of `grease::Request` representing the `foo` module, and
/// `Request::Open` is the member of the `Request` enum representing
/// a `ReqOpen` message.
#[macro_export]
macro_rules! make_request(
	($v:ident, $s:path, $e:path) => {
		impl RequestSendable for $v {
			fn wrap(self, reply_to: &::MessageSender) -> ::Message {
				::Message::Request(reply_to.clone(),
								   $s($e(Box::new(self))))
			}
		}
	}
);

/// Implements NonRequestSendable on the given confirmation structure.
/// For example:
///
/// ```ignore
/// mod foo {
/// make_confirmation(CfmOpen, grease::Confirmation::Foo,
/// Confirmation::Open)
/// }
/// ```
///
/// Where `CfmOpen` is the name of a struct, `grease::Confirmation::Foo` is
/// the member of `grease::Confirmation` representing the `foo` module, and
/// `Confirmation::Open` is the member of the `Confirmation` enum
/// representing a `CfmOpen` message.
#[macro_export]
macro_rules! make_confirmation(
	($v:ident, $s:path, $e:path) => {
		impl NonRequestSendable for $v {
			fn wrap(self) -> ::Message {
				::Message::Confirmation($s($e(Box::new(self))))
			}
		}
	}
);

/// Implements NonRequestSendable on the given indication structure.
/// For example:
///
/// ```ignore
/// mod foo {
/// make_indication(IndConnected, grease::Indication::Foo,
/// Indication::Connected)
/// }
/// ```
///
/// Where `IndConnected` is the name of a struct, `grease::Indication::Foo` is
/// the member of the `grease::Indication` enum representing the `foo` module,
/// and `Indication::Connected` is the member of the `Indication`
/// enum representing a `IndConnected` message.
#[macro_export]
macro_rules! make_indication(
	($v:ident, $s:path, $e:path) => {
		impl NonRequestSendable for $v {
			fn wrap(self) -> ::Message {
				::Message::Indication($s($e(Box::new(self))))
			}
		}
	}
);

/// Implements NonRequestSendable on the given response structure.
/// For example:
///
/// ```ignore
/// mod foo {
///     make_response(RspConnected, grease::Response::Foo, Response::Connected)
/// }
/// ```
///
/// Where `RspConnected` is the name of a struct, `grease::Response::Foo` is
/// the member of `grease::Response` representing the `foo` module, and
/// `Response::Connected` is the member of the `Response` enum representing a
/// `RspConnected` message.
#[macro_export]
macro_rules! make_response(
	($v:ident, $s:path, $e:path) => {
		impl NonRequestSendable for $v {
			fn wrap(self) -> ::Message {
				::Message::Response($s($e(Box::new(self))))
			}
		}
	}
);

// ****************************************************************************
//
// Crates
//
// ****************************************************************************

#[macro_use]
extern crate log;
extern crate mio;
extern crate multi_map;
extern crate rushttp;
#[cfg(test)]
extern crate rand;

// ****************************************************************************
//
// Sub-modules
//
// ****************************************************************************

pub mod app;
pub mod http;
pub mod prelude;
pub mod socket;
pub mod webserv;

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use std::sync::mpsc;
use std::thread;
// use std::time::Duration;

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
	pub reply_to: MessageSender,
	pub context: Context,
}

/// A message is the fundamental unit we pass between tasks.
/// All messages have a body, but requests also have an `MessageSender`
/// object that should be used to send the confirmation in reply.
#[derive(Debug)]
pub enum Message {
	Request(MessageSender, Request),
	Confirmation(Confirmation),
	Indication(Indication),
	Response(Response),
}

/// The set of all requests in the system. This is an enumeration of all the
/// services that can handle requests. The enum included within each service is
/// probably defined in that service's module.
#[derive(Debug)]
pub enum Request {
	Generic(GenericReq),
	Socket(socket::Request),
	Http(http::Request),
	WebServ(webserv::Request),
}

/// The set of all confirmations in the system. This should look exactly like
/// `Request` but as `CfmXXX` instead of `ReqXXX`. These are handled by tasks
/// that send requests - you send a request and you get a confirmation back.
#[derive(Debug)]
pub enum Confirmation {
	Generic(GenericCfm),
	Socket(socket::Confirmation),
	Http(http::Confirmation),
	WebServ(webserv::Confirmation),
}

/// The set of all indications in the system. This is an enumeration of all the
/// services that can send indications. The enum included within each
/// service is probably defined in that service's module.
#[derive(Debug)]
pub enum Indication {
	Socket(socket::Indication),
	Http(http::Indication),
	WebServ(webserv::Indication),
}

/// The set of all responses in the system. This is an enumeration of all the
/// services that need responses (which is actually quite rare). The enum
/// included within each service is probably defined in that service's module.
#[derive(Debug)]
pub enum Response {
	Socket(socket::Response),
}

/// Generic requests should be handled by every task.
#[derive(Debug)]
pub enum GenericReq {
	Ping(Box<PingReq>),
}

/// There is exactly one `GenericCfm` for every `GenericReq`. These should be
/// handled by every task that can ever send a `GenericReq`.
#[derive(Debug)]
pub enum GenericCfm {
	Ping(Box<PingCfm>),
}

/// A simple ping - receiving task should send a `PingCfm` in reply
#[derive(Debug)]
pub struct PingReq {
	/// This will be reflected in the `PingCfm` to aid message association
	pub context: Context,
}

make_request!(PingReq, Request::Generic, GenericReq::Ping);

/// Reply to a `PingReq`
#[derive(Debug)]
pub struct PingCfm {
	/// This is reflected from the `PingReq` to aid message association
	pub context: Context,
}

make_confirmation!(PingCfm, Confirmation::Generic, GenericCfm::Ping);

/// Should be implemented by tasks which handle GenericReq
pub trait GenericProvider {
	fn handle_generic_req(&mut self, req: &GenericReq, reply_to: &MessageSender) {
		match *req {
			GenericReq::Ping(ref x) => {
				let cfm = PingCfm { context: x.context };
				reply_to.send_nonrequest(cfm);
			}
		}
	}
}

/// Implementors of the NonRequestSendable trait can be easily wrapped in a
/// message
/// ready for sending down a MessageSender channel endpoint. All Indication,
/// Confirmation
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

/// Helper function to create a new task.
///
/// As tasks are supposed to live forever, we immediately detach the thread
/// we create by dropping `JoinHandle` returned from `thread::spawn`.
///
/// ```
/// fn main_loop(rx: grease::MessageReceiver, _: grease::MessageSender) {
///     for msg in rx.iter() {
///         match msg {
/// #            _ => { }
///         }
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
	let tb = thread::Builder::new().name(angle_name);
	let _handle = tb.spawn(move || main_loop(receiver, sender_clone));
	return sender;
}

/// Helper function to create a pair of objects for sending and receiving
/// messages.
///
/// Think of them as two ends of a uni-directional pipe, but the sending end of
/// the pipe can be cloned and given to other people, so they can also send
/// things down it.
pub fn make_channel() -> (MessageSender, MessageReceiver) {
	let (tx, rx) = mpsc::channel::<Message>();
	return (MessageSender(tx), MessageReceiver(rx));
}


/// This dumps messages to the logger when they are dropped (i.e. once they have
/// been handled). This means the log should be entirely sufficient to determine
/// what the system has done, which is invaluable for debugging purposes.
///
/// In a future version, this logging might be in binary format over some sort
/// of socket.
impl Drop for Message {
	fn drop(&mut self) {
		debug!("** Destroyed {:?}", self);
	}
}

/// This is the 'receive' end of our message pipe. It wraps up an
/// `mpsc::Sender`, performing a bit of repetitive code required to Box up
/// `RequestSendable` and `NonRequestSendable` messages and wrap them in a
/// covering `Message` enum.
impl MessageSender {
	/// Used for sending requests to a task. The `reply_to` value is a separate
	/// argument because it is mandatory. It would be an error to send a request
	/// without indicating where the matching confirmation should be sent.
	pub fn send_request<T: RequestSendable>(&self, msg: T, reply_to: &MessageSender) {
		self.0.send(msg.wrap(reply_to)).unwrap();
	}

	/// Used for sending confirmations, indications and responses to a task.
	/// There is no `reply_to` argument because these messages do not usually
	/// generate a response. The exception is that some `Indication`s do
	/// generate
	/// a `Response`, but where this is the case, the receiving task will
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

/// This is the 'output' end of our message pipe. It wraps up an
/// `mpsc::Receiver`.
// const MAX_WAIT_SECS:u64 = 10;
impl MessageReceiver {
	// /// Receives with a timeout. Use only for tests - use `iter()` for writing
	// /// a task. Will call panic!() if the timeout is reached.
	// pub fn recv(&self) -> Message {
	// 	match self.0.recv_timeout(Duration::new(MAX_WAIT_SECS, 0)) {
	// 		Ok(msg) => msg,
	// 		Err(mpsc::RecvTimeoutError::Timeout) => panic!("Timed out receiving message"),
	// 		Err(mpsc::RecvTimeoutError::Disconnected) => panic!("Channel disconnected"),
	// 	}
	// }

	/// Receives a message. Use only for tests - use `iter()` for writing
	/// a task.
	pub fn recv(&self) -> Message {
		match self.0.recv() {
			Ok(msg) => msg,
			Err(_) => panic!("Channel disconnected"),
		}
	}

	/// Will panic!() if the channel is not empty. Use for tests.
	pub fn check_empty(&self) {}

	/// Use for test code only
	pub fn try_recv(&self) -> Result<Message, mpsc::TryRecvError> {
		self.0.try_recv()
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

#[cfg(test)]
mod tests {
	#[test]
	fn test_make_channel() {
		let (tx, rx) = ::make_channel();
		let test_req = ::PingReq { context: 1234 };
		tx.send_request(test_req, &tx);
		let msg = rx.recv();
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
