//! # Grease - Tasks and message-passing in Rust.
//!
//! Copyright (c) Cambridge Consultants 2018.
//!
//! Dual MIT/Apache 2.0 licensed. See the top-level COPYRIGHT file for further
//! information and licensing.
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
//! Each task should be in its own module, and it should implement some sort
//! of init function (usually called `make_task`). This will make the message
//! queue (of the appropriate type), spin up a thread to process messages on
//! that queue, and return a handle which may be used to submit messages to
//! that queue. If the task needs to use other tasks, it should take that
//! task's handle as an input - it is therefore important to create your tasks
//! in a bottom up fashion, and not to create any circular dependencies
//! between your tasks!
//!
//! Look in the `grease-http` and `grease-socket` examples directory to see a
//! TCP echo-server example and a very basic HTTP server example.
//!
//! ## Handling incoming messages
//!
//! To handle incoming messages you need to implement the `ServiceProvider`
//! trait for your own messages, and the `ServiceUser` trait for any messages
//! you wish to handle from lower layers. You can do this with an enum and a
//! channel, and then implementing the traits so that they pack the message
//! into your enum wrapper and then stuff it in to the queue.
//!
//! ```
//! #[macro_use] extern crate grease;
//! use std::sync::mpsc;
//!
//! mod foo {
//! 	use super::grease;
//! 	pub struct Service;
//! 	impl grease::Service for Service {
//! 		type Request = ();
//! 		type Confirm = Confirm;
//! 		type Indication = Indication;
//! 		type Response = ();
//! 	}
//! 	pub enum Confirm {}
//! 	pub enum Indication {}
//! }
//!
//! mod bar {
//! 	use super::grease;
//! 	pub struct Service;
//! 	impl grease::Service for Service {
//! 		type Request = ();
//! 		type Confirm = Confirm;
//! 		type Indication = Indication;
//! 		type Response = ();
//! 	}
//! 	pub enum Confirm {}
//! 	pub enum Indication {}
//! }
//!
//! pub struct Service;
//! impl grease::Service for Service {
//! 	type Request = Request;
//! 	type Confirm = Confirm;
//! 	type Indication = Indication;
//! 	type Response = Response;
//! }
//!
//! pub enum Request {}
//! pub enum Confirm {}
//! pub enum Indication {}
//! pub enum Response {}
//!
//! enum Incoming {
//! 	FooCfm(<foo::Service as grease::Service>::Confirm),
//! 	FooInd(<foo::Service as grease::Service>::Indication),
//! 	BarCfm(<bar::Service as grease::Service>::Confirm),
//! 	BarInd(<bar::Service as grease::Service>::Indication),
//! 	Request(Request, grease::ServiceUserHandle<Service>),
//! 	Response(Response),
//! }
//!
//! struct Handle(mpsc::Sender<Incoming>);
//!
//! impl grease::ServiceProvider<Service> for Handle {
//! 	fn send_request(&self, msg: <Service as grease::Service>::Request, reply_to: &grease::ServiceUser<Service>) {
//! 		self.0.send(Incoming::Request(msg, reply_to.clone())).unwrap();
//! 	}
//! 	fn send_response(&self, msg: <Service as grease::Service>::Response) {
//! 		self.0.send(Incoming::Response(msg)).unwrap();
//! 	}
//! 	fn clone(&self) -> grease::ServiceProviderHandle<Service> {
//! 		Box::new(Handle(self.0.clone()))
//! 	}
//! }
//!
//! impl grease::ServiceUser<foo::Service> for Handle {
//! 	fn send_confirm(&self, msg: <foo::Service as grease::Service>::Confirm) {
//! 		self.0.send(Incoming::FooCfm(msg)).unwrap();
//! 	}
//! 	fn send_indication(&self, msg: <foo::Service as grease::Service>::Indication) {
//! 		self.0.send(Incoming::FooInd(msg)).unwrap();
//! 	}
//! 	fn clone(&self) -> grease::ServiceUserHandle<foo::Service> {
//! 		Box::new(Handle(self.0.clone()))
//! 	}
//! }
//!
//! impl grease::ServiceUser<bar::Service> for Handle {
//! 	fn send_confirm(&self, msg: <bar::Service as grease::Service>::Confirm) {
//! 		self.0.send(Incoming::BarCfm(msg)).unwrap();
//! 	}
//! 	fn send_indication(&self, msg: <bar::Service as grease::Service>::Indication) {
//! 		self.0.send(Incoming::BarInd(msg)).unwrap();
//! 	}
//! 	fn clone(&self) -> grease::ServiceUserHandle<bar::Service> {
//! 		Box::new(Handle(self.0.clone()))
//! 	}
//! }
//!
//! # fn main() { }
//! ```
//!
//! Now that's quite a lot of boilerplate and handle-turning, so there is a
//! macro called `service_map!` which can implement all the the above
//! automatically. It simply requires that your messages are named `Request`,
//! `Confirm`, `Indication` and `Response`, and that the handle type you
//! specify is a tuple-struct where the first item has a `send` method which
//! takes the auto-generated type.
//!
//! ```
//! #[macro_use] extern crate grease;
//! use std::sync::mpsc;
//!
//! mod foo {
//! 	use super::grease;
//! 	pub struct Service;
//! 	impl grease::Service for Service {
//! 		type Request = ();
//! 		type Confirm = Confirm;
//! 		type Indication = Indication;
//! 		type Response = ();
//! 	}
//! 	pub enum Confirm {}
//! 	pub enum Indication {}
//! }
//!
//! mod bar {
//! 	use super::grease;
//! 	pub struct Service;
//! 	impl grease::Service for Service {
//! 		type Request = ();
//! 		type Confirm = Confirm;
//! 		type Indication = Indication;
//! 		type Response = ();
//! 	}
//! 	pub enum Confirm {}
//! 	pub enum Indication {}
//! }
//!
//! pub struct Service;
//! impl grease::Service for Service {
//! 	type Request = Request;
//! 	type Confirm = Confirm;
//! 	type Indication = Indication;
//! 	type Response = Response;
//! }
//!
//! pub enum Request {}
//! pub enum Confirm {}
//! pub enum Indication {}
//! pub enum Response {}
//!
//! pub struct Handle(mpsc::Sender<Incoming>);
//!
//! service_map! {
//! 	generate: Incoming,
//! 	service: Service,
//! 	handle: Handle,
//! 	used: {
//! 		foo: (Service, FooCfm, FooInd),
//! 		bar: (Service, BarCfm, BarInd)
//! 	}
//! }
//!
//! # fn main() { }
//! ```
//!
//! Those two code blocks are entirely equivalent, but the macro is a lot
//! shorter! The reason we don't generate the `Handle` is that you might
//! wish to use a different sort of queue.
//!
//! ```ignore
//! pub struct Handle(mio_more::channel::Sender<Incoming>);
//!
//! service_map! {
//! 	generate: Incoming,
//! 	service: Service,
//! 	handle: Handle,
//! 	used: {
//! 		foo: (Service, FooCfm, FooInd),
//! 		bar: (Service, BarCfm, BarInd)
//! 	}
//! }
//! ```
//!
//! If you are writing an application which uses services but does not
//! provide its own, you can use `app_map!`.
//!
//! ```
//! #[macro_use] extern crate grease;
//! use std::sync::mpsc;
//!
//! mod foo {
//! 	use super::grease;
//! 	pub struct Service;
//! 	impl grease::Service for Service {
//! 		type Request = ();
//! 		type Confirm = Confirm;
//! 		type Indication = Indication;
//! 		type Response = ();
//! 	}
//! 	pub enum Confirm {}
//! 	pub enum Indication {}
//! }
//!
//! mod bar {
//! 	use super::grease;
//! 	pub struct Service;
//! 	impl grease::Service for Service {
//! 		type Request = ();
//! 		type Confirm = Confirm;
//! 		type Indication = Indication;
//! 		type Response = ();
//! 	}
//! 	pub enum Confirm {}
//! 	pub enum Indication {}
//! }
//!
//! pub struct Handle(mpsc::Sender<Incoming>);
//!
//! app_map! {
//! 	generate: Incoming,
//! 	handle: Handle,
//! 	used: {
//! 		foo: (Service, FooCfm, FooInd),
//! 		bar: (Service, BarCfm, BarInd)
//! 	}
//! }
//! # fn main() { }
//! ```

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

/// This is the trait for a Service.
/// It's basically a wrapper for the four associated types.
pub trait Service {
	type Request;
	type Confirm;
	type Indication;
	type Response;
}

/// This is the trait for a Service Provider.
///
/// A Service Provider can receive requests and responses. These are given the
/// types `Self::Request` and `Self::Response` respectively. They can be any
/// type, but typically they are tagged enumerations where each tag is a
/// different message.
///
/// The `clone` function returns the boxed trait, as the trait must be object
/// safe - that is, it cannot refer to `Self` (which the normal implementation
/// of `clone` would naturally do).
pub trait ServiceProvider<T>: Send
where
	T: Service,
{
	/// Call this to send a request to this provider.
	fn send_request(&self, req: T::Request, reply_to: &ServiceUser<T>);
	/// Call this to send a response to this provider.
	fn send_response(&self, rsp: T::Response);
	/// Call this to clone this object so another task can use it.
	fn clone(&self) -> ServiceProviderHandle<T>;
}

/// A boxed trait object, which a user can use to send messages in to a
/// provider.
pub type ServiceProviderHandle<T> = Box<ServiceProvider<T>>;

/// A Service User consumes the service provided by a Service Provider.
///
/// This means it must handle Indications and Confirms. It must also be
/// cloneable, so that we can keep copies for use later (with subsequent
/// indications, for example).
///
/// The `clone` function returns the boxed trait, as the trait must be object
/// safe - that is, it cannot refer to `Self` (which the normal implementation
/// of `clone` would naturally do).
pub trait ServiceUser<T>: Send
where
	T: Service,
{
	/// Call this to send a confirmation back to the service user.
	fn send_confirm(&self, cfm: T::Confirm);
	/// Call this to send an indication to the service user.
	fn send_indication(&self, ind: T::Indication);
	/// Call this so we can store this user reference in two places.
	fn clone(&self) -> ServiceUserHandle<T>;
}

/// A boxed trait object, which the provider can use to send messages back to
/// the user.
pub type ServiceUserHandle<T> = Box<ServiceUser<T>>;

#[macro_export]
macro_rules! make_wrapper(
	($v:ident, $s:path, $e:path) => {
		impl std::convert::Into<$s> for $v {
			fn into(self) -> $s {
				$e(self)
			}
		}
	}
);

#[macro_export]
macro_rules! impl_user {
	($handle_type:ident, $n:ident, $svc:path, $cfm_wrapper:ident, $ind_wrapper:ident) => {
		impl $crate::ServiceUser<$svc> for $handle_type {
			fn send_confirm(&self, msg: <$svc as $crate::Service>::Confirm) {
				self.0.send($n::$cfm_wrapper(msg)).unwrap();
			}
			fn send_indication(&self, msg: <$svc as $crate::Service>::Indication) {
				self.0.send($n::$ind_wrapper(msg)).unwrap();
			}
			fn clone(&self) -> $crate::ServiceUserHandle<$svc> {
				::std::boxed::Box::new($handle_type(::std::clone::Clone::clone(&self.0)))
			}
		}
	}
}

#[macro_export]
macro_rules! app_map {
	(
		generate: $n:ident,
		handle: $handle_type:ident,
		used: {
			$( $used_mod:ident: ($used_service:ident, $cfm_wrapper:ident, $ind_wrapper:ident) ),*
		}
	) => {
		enum $n {
			$(
				$cfm_wrapper(<$used_mod::$used_service as $crate::Service>::Confirm),
				$ind_wrapper(<$used_mod::$used_service as $crate::Service>::Indication),
			)*
		}

		$(
			impl_user!($handle_type, $n, $used_mod::$used_service, $cfm_wrapper, $ind_wrapper);
		)*
	}
}

#[macro_export]
macro_rules! service_map {
	(
		generate: $n:ident,
		service: $our_svc:ident,
		handle: $handle_type:ident,
		used: {
			$( $used_mod:ident: ($used_service:ident, $cfm_wrapper:ident, $ind_wrapper:ident) ),*
		}
	) => {

		enum $n {
			Request(<$our_svc as $crate::Service>::Request, $crate::ServiceUserHandle<$our_svc>),
			Response(<$our_svc as $crate::Service>::Response),
			$(
				$cfm_wrapper(<$used_mod::$used_service as $crate::Service>::Confirm),
				$ind_wrapper(<$used_mod::$used_service as $crate::Service>::Indication),
			)*
		}

		impl $crate::ServiceProvider<$our_svc> for $handle_type {
			fn send_request(&self, msg: <$our_svc as grease::Service>::Request, reply_to: &$crate::ServiceUser<$our_svc>) {
				self.0.send($n::Request(msg, reply_to.clone())).unwrap();
			}
			fn send_response(&self, msg: <$our_svc as grease::Service>::Response) {
				self.0.send($n::Response(msg)).unwrap();
			}
			fn clone(&self) -> $crate::ServiceProviderHandle<$our_svc> {
				::std::boxed::Box::new($handle_type(::std::clone::Clone::clone(&self.0)))
			}
		}

		$(
			impl_user!($handle_type, $n, $used_mod::$used_service, $cfm_wrapper, $ind_wrapper);
		)*
	}
}

/// A type used to passing context between layers. If each layer maintains
/// a `HashMap<Context, T>`, when a confirmation comes back from the layer
/// below, it's easy to work out which T it corresponds to.
/// TODO: Replace this with a trait and a macro that generates a newtype
/// which implements the trait.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct Context(usize);

/// When handling a request, the process may take some time. As the request
/// must be destroyed as soon as it arrives (for logging purposes), the
/// essential details are recorded so that a Confirmation can be sent at a
/// later date.
pub struct ReplyContext<T> {
	pub reply_to: ServiceUserHandle<T>,
	pub context: Context,
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

impl Context {
	pub fn new(value: usize) -> Context {
		Context(value)
	}

	pub fn as_usize(&self) -> usize {
		self.0
	}

	/// You can use take to grab a copy of the current value,
	/// while incrementing it ready for the next use.
	pub fn take(&mut self) -> Context {
		let result = Context(self.0);
		self.0 = self.0.wrapping_add(1);
		result
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
