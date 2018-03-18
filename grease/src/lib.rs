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
//! Messages in grease are defined by each individual layer in four different types:
//!
//! * Request
//! * Confirm
//! * Indication, and
//! * Response.
//!
//! Typically each of these will be an enum of all the different messages of
//! that type but this is up to each layer to define. Each layer must then
//! provide an object which implements `ServiceProvider` - typically this is a
//! thin wrapper around an `mpsc::Channel` which accepts an `enum` of all the
//! messages a task can receive; that is, all of its own `Request` and
//! `Response` messages, plus all of the `Indication` and `Confirm` messages
//! it could receive from any other Service Providers that it makes use of.
//! The task should also implement `ServiceUser` on that channel wrapper
//! for each of those used services.
//!
//! See the `http` module for an example - it uses the `socket` module.
//!
//! ## Implementing a task
//!
//! TODO

// ****************************************************************************
//
// Macros
//
// ****************************************************************************

// None

// ****************************************************************************
//
// Crates
//
// ****************************************************************************

// None

// ****************************************************************************
//
// Sub-modules
//
// ****************************************************************************

pub mod prelude;

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

// None

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// This is the trait for a Service Provider.
///
/// A Service Provider can receive requests and responses. These are given the
/// types `Self::Request` and `Self::Response` respectively. They can be any
/// type, but typically they are tagged enumerations where each tag is a
/// different message.
pub trait ServiceProvider<REQ, CFM, IND, RSP> {
	/// Call this to send a request to this provider.
	fn send_request(&self, req: REQ, reply_to: UserHandle<CFM, IND>);
	fn send_response(&self, rsp: RSP);
}

/// A Service User consumes the service provided by a Service Provider.
///
/// This means it must handle Indications and Confirms. It must also be
/// cloneable, so that we can keep copies for use later (with subsequent
/// indications, for example).
pub trait ServiceUser<CFM, IND> {
	fn send_confirm(&self, cfm: CFM);
	fn send_indication(&self, ind: IND);
	fn clone(&self) -> UserHandle<CFM, IND>;
}

/// A boxed trait object, which the provider can use to send messages
/// back to the user.
pub type UserHandle<CFM, IND> = Box<ServiceUser<CFM, IND> + Send>;

/// A type used to passing context between layers. If each layer maintains
/// a HashMap<Context, T>, when a confirmation comes back from the layer
/// below, it's easy to work out which T it corresponds to.
/// TODO: Replace this with a trait and a macro that generates a newtype
/// which implements the trait.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct Context(usize);

impl Context {
	pub fn new(value: usize) -> Context {
		Context(value)
	}

	pub fn as_usize(&self) -> usize {
		self.0
	}
}

impl Default for Context {
	fn default() -> Context {
		Context(0)
	}
}

impl std::fmt::Display for Context {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "Context({})", self.0)
	}
}

impl Context {
	/// You can use take to grab a copy of the current value,
	/// while incrementing it ready for the next use.
	pub fn take(&mut self) -> Context {
		let result = Context(self.0);
		self.0 = self.0.wrapping_add(1);
		result
	}
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
