//! # socket - A TCP server task
//!
//! The grease socket task makes handling sockets easier. A user requests the
//! socket task opens a new socket and it does so (if possible). The user then
//! receives asynchronous indications when data arrives on the socket and/or
//! when the socket closes.

#![deny(missing_docs)]

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use std::collections::{HashMap, VecDeque};
use std::convert::From;
use std::fmt;
use std::io::prelude::*;
use std::io;
use std::net;
use std::thread;

use mio;

use ::prelude::*;

// ****************************************************************************
//
// Public Messages
//
// ****************************************************************************

/// Requests that can be sent to the Socket task
/// We box all the parameters, in case any of the structs are large as we don't
/// want to bloat the master Message type.
#[derive(Debug)]
pub enum Request {
	/// A Bind Request - Bind a listen socket
	Bind(Box<ReqBind>),
	/// An Unbind Request - Unbind a bound listen socket
	Unbind(Box<ReqUnbind>),
	/// A Close request - Close an open connection
	Close(Box<ReqClose>),
	/// A Send request - Send something on a connection
	Send(Box<ReqSend>),
}

/// Confirmations sent from the Socket task in answer to a Request
#[derive(Debug)]
pub enum Confirmation {
	/// A Bind Confirm - Bound a listen socket
	Bind(Box<CfmBind>),
	/// An Unbind Confirm - Unbound a bound listen socket
	Unbind(Box<CfmUnbind>),
	/// A Close Confirm - Closed an open connection
	Close(Box<CfmClose>),
	/// A Send Confirm - Sent something on a connection
	Send(Box<CfmSend>),
}

/// Asynchronous indications sent by the Socket task
#[derive(Debug)]
pub enum Indication {
	/// A Connected Indication - Indicates that a listening socket has been
	/// connected to
	Connected(Box<IndConnected>),
	/// A Dropped Indication - Indicates that an open socket has been dropped
	Dropped(Box<IndDropped>),
	/// A Received Indication - Indicates that data has arrived on an open
	/// socket
	Received(Box<IndReceived>),
}

/// Responses to Indications required
#[derive(Debug)]
pub enum Response {
	/// a Receieved Response - unblocks the open socket so more IndReceived can
	/// be sent
	Received(Box<RspReceived>),
}

/// Bind a listen socket
#[derive(Debug)]
pub struct ReqBind {
	/// The address to bind to
	pub addr: net::SocketAddr,
	/// Reflected in the cfm
	pub context: ::Context,
}

make_request!(ReqBind, ::Request::Socket, Request::Bind);

/// Unbind a bound listen socket
#[derive(Debug)]
pub struct ReqUnbind {
	/// The handle from a CfmBind
	pub handle: ConnHandle,
	/// Reflected in the cfm
	pub context: ::Context,
}

make_request!(ReqUnbind, ::Request::Socket, Request::Unbind);

/// Close an open connection
#[derive(Debug)]
pub struct ReqClose {
	/// The handle from a IndConnected
	pub handle: ConnHandle,
	/// Reflected in the cfm
	pub context: ::Context,
}

make_request!(ReqClose, ::Request::Socket, Request::Close);

/// Send something on a connection
pub struct ReqSend {
	/// The handle from a CfmBind
	pub handle: ConnHandle,
	/// Some (maybe) unique identifier
	pub context: ::Context,
	/// The data to be sent
	pub data: Vec<u8>,
}

make_request!(ReqSend, ::Request::Socket, Request::Send);

/// Reply to a ReqBind.
#[derive(Debug)]
pub struct CfmBind {
	/// Either a new ListenHandle or an error
	pub result: Result<ListenHandle, SocketError>,
	/// Reflected from the req
	pub context: ::Context,
}

make_confirmation!(CfmBind, ::Confirmation::Socket, Confirmation::Bind);

/// Reply to a ReqUnbind.
#[derive(Debug)]
pub struct CfmUnbind {
	/// The handle requested for unbinding
	pub handle: ListenHandle,
	/// Whether we were successful in unbinding
	pub result: Result<(), SocketError>,
	/// Reflected from the req
	pub context: ::Context,
}

make_confirmation!(CfmUnbind, ::Confirmation::Socket, Confirmation::Unbind);

/// Reply to a ReqClose. Will flush out all
/// existing data.
#[derive(Debug)]
pub struct CfmClose {
	/// The handle requested for closing
	pub handle: ConnHandle,
	/// Success or failed
	pub result: Result<(), SocketError>,
	/// Reflected from the req
	pub context: ::Context,
}

make_confirmation!(CfmClose, ::Confirmation::Socket, Confirmation::Close);

/// Reply to a ReqSend. The data has not necessarily
/// been sent, but it is safe to send some more data.
#[derive(Debug)]
pub struct CfmSend {
	/// The handle requested for sending
	pub handle: ConnHandle,
	/// Amount sent or error
	pub result: Result<usize, SocketError>,
	/// Some (maybe) unique identifier
	pub context: ::Context,
}

make_confirmation!(CfmSend, ::Confirmation::Socket, Confirmation::Send);

/// Indicates that a listening socket has been connected to.
#[derive(Debug)]
pub struct IndConnected {
	/// The listen handle the connection came in on
	pub listen_handle: ListenHandle,
	/// The handle for the new connection
	pub conn_handle: ConnHandle,
	/// Details about who connected
	pub peer: net::SocketAddr,
}

make_indication!(IndConnected, ::Indication::Socket, Indication::Connected);

/// Indicates that a socket has been dropped.
#[derive(Debug)]
pub struct IndDropped {
	/// The handle that is no longer valid
	pub handle: ConnHandle,
}

make_indication!(IndDropped, ::Indication::Socket, Indication::Dropped);

/// Indicates that data has arrived on the socket
/// No further data will be sent on this handle until
/// RspReceived is sent back. Note that this type
/// has a custom std::fmt::Debug implementation so it
/// doesn't print the (lengthy) contents of `data`.
pub struct IndReceived {
	/// The handle for the socket data came in on
	pub handle: ConnHandle,
	/// The data that came in (might be a limited to a small amount)
	pub data: Vec<u8>,
}

make_indication!(IndReceived, ::Indication::Socket, Indication::Received);

/// Tell the task that more data can now be sent.
#[derive(Debug)]
pub struct RspReceived {
	/// Which handle is now free to send up more data
	pub handle: ConnHandle,
}

make_response!(RspReceived, ::Response::Socket, Response::Received);

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Users of the socket task should implement this trait to
/// make handling the incoming Confirmation and Indication a little
/// easier.
pub trait User {
	/// Handles a Socket Confirmation, such as you will receive after sending
	/// a Socket Request, by unpacking the enum and routing the struct
	/// contained within to the appropriate handler.
	fn handle_socket_cfm(&mut self, msg: &Confirmation) {
		match *msg {
			Confirmation::Bind(ref x) => self.handle_socket_cfm_bind(&x),
			Confirmation::Unbind(ref x) => self.handle_socket_cfm_unbind(&x),
			Confirmation::Close(ref x) => self.handle_socket_cfm_close(&x),
			Confirmation::Send(ref x) => self.handle_socket_cfm_send(&x),
		}
	}

	/// Called when a Bind confirmation is received.
	fn handle_socket_cfm_bind(&mut self, msg: &CfmBind);

	/// Called when an Unbind confirmation is received.
	fn handle_socket_cfm_unbind(&mut self, msg: &CfmUnbind);

	/// Called when a Close confirmation is received.
	fn handle_socket_cfm_close(&mut self, msg: &CfmClose);

	/// Called when a Send confirmation is received.
	fn handle_socket_cfm_send(&mut self, msg: &CfmSend);

	/// Handles a Socket Indication by unpacking the enum and routing the
	/// struct contained withing to the appropriate handler.
	fn handle_socket_ind(&mut self, msg: &Indication) {
		match *msg {
			Indication::Connected(ref x) => self.handle_socket_ind_connected(&x),
			Indication::Dropped(ref x) => self.handle_socket_ind_dropped(&x),
			Indication::Received(ref x) => self.handle_socket_ind_received(&x),
		}
	}

	/// Handles a Connected indication.
	fn handle_socket_ind_connected(&mut self, msg: &IndConnected);

	/// Handles a connection Dropped indication.
	fn handle_socket_ind_dropped(&mut self, msg: &IndDropped);

	/// Handles a data Received indication.
	fn handle_socket_ind_received(&mut self, msg: &IndReceived);
}

/// Uniquely identifies an listening socket
pub type ListenHandle = ::Context;

/// Uniquely identifies an open socket
pub type ConnHandle = ::Context;

/// All possible errors the Socket task might want to
/// report.
#[derive(Debug, Copy, Clone)]
pub enum SocketError {
	/// An underlying socket error
	IOError(io::ErrorKind),
	/// The given handle was not recognised
	BadHandle,
	/// The pending write failed because the socket dropped
	Dropped,
	/// Function not implemented yet
	NotImplemented,
}

// ****************************************************************************
//
// Private Types
//
// ****************************************************************************

/// Created for every bound (i.e. listening) socket
struct ListenSocket {
	handle: ListenHandle,
	ind_to: ::MessageSender,
	listener: mio::tcp::TcpListener,
}

/// Create for every pending write
struct PendingWrite {
	context: ::Context,
	sent: usize,
	data: Vec<u8>,
	reply_to: ::MessageSender,
}

/// Created for every connection receieved on a ListenSocket
struct ConnectedSocket {
	// parent: ListenHandle,
	ind_to: ::MessageSender,
	handle: ConnHandle,
	connection: mio::tcp::TcpStream,
	/// There's a read the user hasn't process yet
	outstanding: bool,
	/// Queue of pending writes
	pending_writes: VecDeque<PendingWrite>,
}

/// One instance per task. Stores all the task data.
struct TaskContext {
	/// Set of all bound sockets
	listeners: HashMap<ListenHandle, ListenSocket>,
	/// Set of all connected sockets
	connections: HashMap<ConnHandle, ConnectedSocket>,
	/// The next handle we'll use for a bound/open socket
	next_handle: ::Context,
	/// The special channel our messages arrive on
	mio_rx: mio::channel::Receiver<::Message>,
	/// The object we poll on
	poll: mio::Poll,
}

// ****************************************************************************
//
// Public Data
//
// ****************************************************************************

// None

// ****************************************************************************
//
// Private Data
//
// ****************************************************************************

const MAX_READ_LEN: usize = 128;
const MESSAGE_TOKEN: mio::Token = mio::Token(0);

// ****************************************************************************
//
// Public Functions
//
// ****************************************************************************

/// Creates a new socket task. Returns an object that can be used
/// to send this task messages.
pub fn make_task() -> ::MessageSender {
	::make_task("socket", main_loop)
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

/// The task runs this main loop indefinitely.
/// Unfortunately, to use mio, we have to use their special
/// channels. So, we spin up a thread to bounce from one
/// channel to the other. We don't need our own
/// MessageSender as we don't send Requests that need replying to.
fn main_loop(grease_rx: ::MessageReceiver, _: ::MessageSender) {
	let (mio_tx, mio_rx) = mio::channel::channel();
	let _ = thread::spawn(move || {
		for msg in grease_rx.iter() {
			let _ = mio_tx.send(msg);
		}
	});
	let mut task_context = TaskContext::new(mio_rx);
	loop {
		task_context.poll();
	}
}

impl TaskContext {
	fn poll(&mut self) {
		let mut events = mio::Events::with_capacity(1024);
		let num_events = self.poll.poll(&mut events, None).unwrap();
		trace!("Woke up! Handling num_events={}", num_events);
		for event in events.iter() {
			self.ready(event.token(), event.kind())
		}
	}

	/// Called when mio has an update on a registered listener or connection
	/// We have to check the Ready to find out whether our socket is
	/// readable or writable
	fn ready(&mut self, token: mio::Token, ready: mio::Ready) {
		debug!("ready={:?}, token={:?}!", ready, token);
		let handle = token.0;
		if ready.is_readable() {
			if token == MESSAGE_TOKEN {
				let msg = self.mio_rx.try_recv().unwrap();
				self.handle_message(msg);
			} else if self.listeners.contains_key(&handle) {
				debug!("Readable listen socket {}?", handle);
				self.accept(handle)
			} else if self.connections.contains_key(&handle) {
				debug!("Readable connected socket {}", handle);
				self.read(handle)
			} else {
				warn!("Readable on unknown token {}", handle);
			}
		}
		if ready.is_writable() {
			if self.listeners.contains_key(&handle) {
				debug!("Writable listen socket {}?", handle);
			} else if self.connections.contains_key(&handle) {
				debug!("Writable connected socket {}", handle);
				self.pending_writes(handle);
			} else {
				warn!("Writable on unknown token {}", handle);
			}
		}
		if ready.is_error() {
			if self.listeners.contains_key(&handle) {
				debug!("Error listen socket {}", handle);
			} else if self.connections.contains_key(&handle) {
				debug!("Error connected socket {}", handle);
				self.dropped(handle);
			} else {
				warn!("Error on unknown token {}", handle);
			}
		}
		if ready.is_hup() {
			if self.listeners.contains_key(&handle) {
				warn!("HUP listen socket {} is not handled.", handle);
			} else if self.connections.contains_key(&handle) {
				debug!("HUP connected socket {}", handle);
				self.dropped(handle);
			} else {
				warn!("HUP on unknown token {}", handle);
			}
		}
		debug!("Ready is done");
	}

	/// Called when our task has received a Message
	fn handle_message(&mut self, msg: ::Message) {
		debug!("Notify!");
		match msg {
			// We only handle our own requests and responses
			::Message::Request(ref reply_to, ::Request::Socket(ref x)) => {
				self.handle_socket_req(x, reply_to)
			}
			::Message::Response(::Response::Socket(ref x)) => self.handle_socket_rsp(x),
			// We don't expect any Indications or Confirmations from other providers
			// If we get here, someone else has made a mistake
			_ => error!("Unexpected message in socket task: {:?}", msg),
		}
		debug!("Notify is done");
	}

	/// Init the context
	pub fn new(mio_rx: mio::channel::Receiver<::Message>) -> TaskContext {
		let t = TaskContext {
			listeners: HashMap::new(),
			connections: HashMap::new(),
			next_handle: MESSAGE_TOKEN.0 + 1,
			mio_rx: mio_rx,
			poll: mio::Poll::new().unwrap(),
		};
		t.poll
			.register(&t.mio_rx,
			          MESSAGE_TOKEN,
			          mio::Ready::readable(),
			          mio::PollOpt::level())
			.unwrap();
		t
	}

	/// Accept a new incoming connection and let the user know
	/// with a IndConnected
	fn accept(&mut self, ls_handle: ListenHandle) {
		// We know this exists because we checked it before we got here
		let ls = self.listeners.get(&ls_handle).unwrap();
		if let Ok((stream, conn_addr)) = ls.listener.accept() {
			let cs = ConnectedSocket {
				// parent: ls.handle,
				handle: self.next_handle,
				ind_to: ls.ind_to.clone(),
				connection: stream,
				outstanding: false,
				pending_writes: VecDeque::new(),
			};
			self.next_handle += 1;
			self.poll
				.register(&cs.connection,
				          mio::Token(cs.handle),
				          mio::Ready::readable() | mio::Ready::writable() | mio::Ready::hup() |
				          mio::Ready::error(),
				          mio::PollOpt::edge())
				.unwrap();
			let ind = IndConnected {
				listen_handle: ls.handle,
				conn_handle: cs.handle,
				peer: conn_addr,
			};
			self.connections.insert(cs.handle, cs);
			ls.ind_to.send_nonrequest(ind);
		} else {
			warn!("accept returned None!");
		}
	}

	/// Data can be sent a connected socket. Send what we have
	fn pending_writes(&mut self, cs_handle: ConnHandle) {
		// We know this exists because we checked it before we got here
		let mut cs = self.connections.get_mut(&cs_handle).unwrap();
		loop {
			if let Some(mut pw) = cs.pending_writes.pop_front() {
				let to_send = pw.data.len() - pw.sent;
				match cs.connection.write(&pw.data[pw.sent..]) {
					Ok(len) if len < to_send => {
						let left = to_send - len;
						debug!("Sent {} of {}, leaving {}", len, to_send, left);
						pw.sent = pw.sent + len;
						cs.pending_writes.push_front(pw);
						// No cfm here - we wait some more
						break;
					}
					Ok(_) => {
						debug!("Sent all {}", to_send);
						pw.sent = pw.sent + to_send;
						let cfm = CfmSend {
							handle: cs.handle,
							context: pw.context,
							result: Ok(pw.sent),
						};
						pw.reply_to.send_nonrequest(cfm);
					}
					Err(err) => {
						warn!("Send error: {}", err);
						let cfm = CfmSend {
							handle: cs.handle,
							context: pw.context,
							result: Err(err.into()),
						};
						pw.reply_to.send_nonrequest(cfm);
						break;
					}
				}
			} else {
				break;
			}
		}
	}

	/// Data is available on a connected socket. Pass it up
	fn read(&mut self, cs_handle: ConnHandle) {
		debug!("Reading connection {}", cs_handle);
		// We know this exists because we checked it before we got here
		let mut cs = self.connections.get_mut(&cs_handle).unwrap();
		// Only pass up one indication at a time
		if !cs.outstanding {
			// Cap the max amount we will read
			let mut buffer = vec![0u8; MAX_READ_LEN];
			match cs.connection.read(buffer.as_mut_slice()) {
				Ok(0) => {
					debug!("Read nothing");
				}
				Ok(len) => {
					debug!("Read {} octets", len);
					let _ = buffer.split_off(len);
					let ind = IndReceived {
						handle: cs.handle,
						data: buffer,
					};
					cs.outstanding = true;
					cs.ind_to.send_nonrequest(ind);
				}
				Err(_) => {}
			}
		} else {
			debug!("Not reading - outstanding ind")
		}
	}

	/// Connection has gone away. Clean up.
	fn dropped(&mut self, cs_handle: ConnHandle) {
		// We know this exists because we checked it before we got here
		let cs = self.connections.remove(&cs_handle).unwrap();
		self.poll.deregister(&cs.connection).unwrap();
		let ind = IndDropped { handle: cs_handle };
		cs.ind_to.send_nonrequest(ind);
	}

	/// Handle requests
	pub fn handle_socket_req(&mut self, req: &Request, reply_to: &::MessageSender) {
		debug!("request: {:?}", req);
		match *req {
			Request::Bind(ref x) => self.handle_bind(x, reply_to),
			Request::Unbind(ref x) => self.handle_unbind(x, reply_to),
			Request::Close(ref x) => self.handle_close(x, reply_to),
			Request::Send(ref x) => self.handle_send(x, reply_to),
		}
	}

	/// Open a new socket with the given parameters.
	fn handle_bind(&mut self, req_bind: &ReqBind, reply_to: &::MessageSender) {
		info!("Binding {}...", req_bind.addr);
		let cfm = match mio::tcp::TcpListener::bind(&req_bind.addr) {
			Ok(server) => {
				let h = self.next_handle;
				self.next_handle += 1;
				debug!("Allocated listen handle {}", h);
				let l = ListenSocket {
					handle: h,
					// We assume any future indications should be sent
					// to the same place we send the CfmBind.
					ind_to: reply_to.clone(),
					listener: server,
				};
				match self.poll.register(&l.listener,
				                         mio::Token(h),
				                         mio::Ready::readable(),
				                         mio::PollOpt::edge()) {
					Ok(_) => {
						self.listeners.insert(h, l);
						CfmBind {
							result: Ok(h),
							context: req_bind.context,
						}
					}
					Err(io_error) => {
						CfmBind {
							result: Err(io_error.into()),
							context: req_bind.context,
						}
					}
				}
			}
			Err(io_error) => {
				CfmBind {
					result: Err(io_error.into()),
					context: req_bind.context,
				}
			}
		};
		reply_to.send_nonrequest(cfm);
	}

	/// Handle a ReqUnbind.
	fn handle_unbind(&mut self, req_unbind: &ReqUnbind, reply_to: &::MessageSender) {
		let cfm = CfmClose {
			result: Err(SocketError::NotImplemented),
			handle: req_unbind.handle,
			context: req_unbind.context,
		};
		reply_to.send_nonrequest(cfm);
	}

	/// Handle a ReqClose
	fn handle_close(&mut self, req_close: &ReqClose, reply_to: &::MessageSender) {
		let mut found = false;
		if let Some(_) = self.connections.remove(&req_close.handle) {
			// Connection closes automatically??
			found = true;
		}
		let cfm = CfmClose {
			result: if found {
				Ok(())
			} else {
				Err(SocketError::BadHandle)
			},
			handle: req_close.handle,
			context: req_close.context,
		};
		reply_to.send_nonrequest(cfm);
	}

	/// Handle a ReqSend
	fn handle_send(&mut self, req_send: &ReqSend, reply_to: &::MessageSender) {
		if let Some(cs) = self.connections.get_mut(&req_send.handle) {
			let to_send = req_send.data.len();
			// Let's see how much we can get rid off right now
			if cs.pending_writes.len() > 0 {
				debug!("Storing write len {}", to_send);
				let pw = PendingWrite {
					sent: 0,
					context: req_send.context,
					data: req_send.data.clone(),
					reply_to: reply_to.clone(),
				};
				cs.pending_writes.push_back(pw);
				// No cfm here - we wait
			} else {
				match cs.connection.write(&req_send.data) {
					Ok(len) if len < to_send => {
						let left = to_send - len;
						debug!("Sent {} of {}, leaving {}", len, to_send, left);
						let pw = PendingWrite {
							sent: len,
							context: req_send.context,
							data: req_send.data.clone(),
							reply_to: reply_to.clone(),
						};
						cs.pending_writes.push_back(pw);
						// No cfm here - we wait
					}
					Ok(_) => {
						debug!("Sent all {}", to_send);
						let cfm = CfmSend {
							context: req_send.context,
							handle: req_send.handle,
							result: Ok(to_send),
						};
						reply_to.send_nonrequest(cfm);
					}
					Err(err) => {
						warn!("Send error: {}", err);
						let cfm = CfmSend {
							context: req_send.context,
							handle: req_send.handle,
							result: Err(err.into()),
						};
						reply_to.send_nonrequest(cfm);
					}
				}
			}
		} else {
			let cfm = CfmSend {
				result: Err(SocketError::BadHandle),
				context: req_send.context,
				handle: req_send.handle,
			};
			reply_to.send_nonrequest(cfm);
		}
	}

	/// Handle responses
	pub fn handle_socket_rsp(&mut self, rsp: &Response) {
		match *rsp {
			Response::Received(ref x) => self.handle_received(x),
		}
	}

	/// Someone wants more data
	fn handle_received(&mut self, rsp_received: &RspReceived) {
		let mut need_read = false;
		// Read response handle might not be valid - it might
		// have crossed over with a disconnect.
		if let Some(cs) = self.connections.get_mut(&rsp_received.handle) {
			cs.outstanding = false;
			// Let's try and send them some - if if exhausts the
			// buffer on the socket, the event loop will automatically
			// set itself to interrupt when more data arrives
			need_read = true;
		}
		if need_read {
			// Try and read it - won't hurt if we can't.
			self.read(rsp_received.handle)
		}
	}
}

impl Drop for ConnectedSocket {
	fn drop(&mut self) {
		for pw in self.pending_writes.iter() {
			let cfm = CfmSend {
				handle: self.handle,
				context: pw.context,
				result: Err(SocketError::Dropped),
			};
			pw.reply_to.send_nonrequest(cfm);
		}
	}
}

#[cfg(test)]
mod test {
	use std::io::prelude::*;
	use std::net;
	use rand;
	use rand::Rng;
	use super::*;

	#[test]
	/// Binds 127.0.1.1:8000
	fn bind_port_ok() {
		let socket_thread = make_task();
		let (reply_to, test_rx) = ::make_channel();
		let bind_req = ReqBind {
			addr: "127.0.1.1:8000".parse().unwrap(),
			context: 1234,
		};
		socket_thread.send_request(bind_req, &reply_to);
		let cfm = test_rx.recv();
		match cfm {
			::Message::Confirmation(::Confirmation::Socket(Confirmation::Bind(ref x))) => {
				assert_eq!(x.context, 1234);
				assert!(x.result.is_ok());
			}
			_ => panic!("Bad match"),
		}
	}

	#[test]
	/// Fails to bind 127.0.1.1:22 (because you need to be root)
	fn bind_port_fail() {
		let socket_thread = make_task();
		let (reply_to, test_rx) = ::make_channel();
		// Shouldn't be able to bind :22 as normal user
		let bind_req = ReqBind {
			addr: "127.0.1.1:22".parse().unwrap(),
			context: 5678,
		};
		socket_thread.send_request(bind_req, &reply_to);
		let cfm = test_rx.recv();
		match cfm {
			::Message::Confirmation(::Confirmation::Socket(Confirmation::Bind(ref x))) => {
				assert_eq!(x.context, 5678);
				assert!(x.result.is_err());
			}
			_ => panic!("Bad match"),
		}
	}

	#[test]
	/// Fails to bind 8.8.8.8:8000 (because you don't have that i/f)
	fn bind_if_fail() {
		let socket_thread = make_task();
		let (reply_to, test_rx) = ::make_channel();
		// Shouldn't be able to bind :22 as normal user
		let bind_req = ReqBind {
			addr: "8.8.8.8:8000".parse().unwrap(),
			context: 6666,
		};
		socket_thread.send_request(bind_req, &reply_to);
		let cfm = test_rx.recv();
		match cfm {
			::Message::Confirmation(::Confirmation::Socket(Confirmation::Bind(ref x))) => {
				assert_eq!(x.context, 6666);
				assert!(x.result.is_err());
			}
			_ => panic!("Bad match"),
		}
	}

	#[test]
	/// Connects to a socket on 127.0.1.1:8001
	fn connect() {
		let socket_thread = make_task();
		let (reply_to, test_rx) = ::make_channel();

		let bind_req = ReqBind {
			addr: "127.0.1.1:8001".parse().unwrap(),
			context: 5678,
		};
		socket_thread.send_request(bind_req, &reply_to);
		let listen_handle = match test_rx.recv() {
			::Message::Confirmation(::Confirmation::Socket(Confirmation::Bind(ref x))) => {
				assert_eq!(x.context, 5678);
				x.result.unwrap()
			}
			_ => panic!("Bad match"),
		};

		// Make a TCP connection
		let stream = net::TcpStream::connect("127.0.1.1:8001").unwrap();

		// Check we get an IndConnected
		let conn_handle = match test_rx.recv() {
			::Message::Indication(::Indication::Socket(Indication::Connected(ref x))) => {
				assert_eq!(x.listen_handle, listen_handle);
				assert_eq!(x.peer, stream.local_addr().unwrap());
				x.conn_handle
			}
			_ => panic!("Bad match"),
		};

		stream.shutdown(net::Shutdown::Both).unwrap();

		// Check we get an IndDropped
		match test_rx.recv() {
			::Message::Indication(::Indication::Socket(Indication::Dropped(ref x))) => {
				assert_eq!(x.handle, conn_handle);
			}
			_ => panic!("Bad match"),
		};
	}

	#[test]
	/// Uses 127.0.1.1:8002 and 127.0.1.1:8003 to send random data
	fn two_connections() {
		let socket_thread = make_task();
		let (reply_to, test_rx) = ::make_channel();

		let bind_req = ReqBind {
			addr: "127.0.1.1:8002".parse().unwrap(),
			context: 5678,
		};
		socket_thread.send_request(bind_req, &reply_to);
		let listen_handle8002 = match test_rx.recv() {
			::Message::Confirmation(::Confirmation::Socket(Confirmation::Bind(ref x))) => {
				assert_eq!(x.context, 5678);
				x.result.unwrap()
			}
			_ => panic!("Bad match"),
		};

		let bind_req = ReqBind {
			addr: "127.0.1.1:8003".parse().unwrap(),
			context: 5678,
		};
		socket_thread.send_request(bind_req, &reply_to);
		let listen_handle8003 = match test_rx.recv() {
			::Message::Confirmation(::Confirmation::Socket(Confirmation::Bind(ref x))) => {
				assert_eq!(x.context, 5678);
				x.result.unwrap()
			}
			_ => panic!("Bad match"),
		};

		// Make two TCP connections
		let stream8003 = net::TcpStream::connect("127.0.1.1:8003").unwrap();
		let stream8002 = net::TcpStream::connect("127.0.1.1:8002").unwrap();

		// Check we get an IndConnected, twice
		let conn_handle8003 = match test_rx.recv() {
			::Message::Indication(::Indication::Socket(Indication::Connected(ref x))) => {
				assert_eq!(x.listen_handle, listen_handle8003);
				assert_eq!(x.peer, stream8003.local_addr().unwrap());
				x.conn_handle
			}
			_ => panic!("Bad match"),
		};
		let conn_handle8002 = match test_rx.recv() {
			::Message::Indication(::Indication::Socket(Indication::Connected(ref x))) => {
				assert_eq!(x.listen_handle, listen_handle8002);
				assert_eq!(x.peer, stream8002.local_addr().unwrap());
				x.conn_handle
			}
			_ => panic!("Bad match"),
		};

		// Shutdown the 8002 connection
		stream8002.shutdown(net::Shutdown::Both).unwrap();

		// Check we get an IndDropped
		match test_rx.recv() {
			::Message::Indication(::Indication::Socket(Indication::Dropped(ref x))) => {
				assert_eq!(x.handle, conn_handle8002);
			}
			_ => panic!("Bad match"),
		};

		// Shutdown the 8003 connection
		stream8003.shutdown(net::Shutdown::Both).unwrap();

		// Check we get an IndDropped
		match test_rx.recv() {
			::Message::Indication(::Indication::Socket(Indication::Dropped(ref x))) => {
				assert_eq!(x.handle, conn_handle8003);
			}
			_ => panic!("Bad match"),
		};
	}

	#[test]
	/// Uses 127.0.1.1:8004 to send random data
	fn send_data() {
		let socket_thread = make_task();
		let (reply_to, test_rx) = ::make_channel();

		let bind_req = ReqBind {
			addr: "127.0.1.1:8004".parse().unwrap(),
			context: 5678,
		};
		socket_thread.send_request(bind_req, &reply_to);
		let listen_handle = match test_rx.recv() {
			::Message::Confirmation(::Confirmation::Socket(Confirmation::Bind(ref x))) => {
				assert_eq!(x.context, 5678);
				x.result.unwrap()
			}
			_ => panic!("Bad match"),
		};

		// Make a TCP connection
		let mut stream = net::TcpStream::connect("127.0.1.1:8004").unwrap();

		// Check we get an IndConnected
		let conn_handle = match test_rx.recv() {
			::Message::Indication(::Indication::Socket(Indication::Connected(ref x))) => {
				assert_eq!(x.listen_handle, listen_handle);
				assert_eq!(x.peer, stream.local_addr().unwrap());
				x.conn_handle
			}
			_ => panic!("Bad match"),
		};

		// Send some data
		let data = rand::thread_rng().gen_iter().take(1024).collect::<Vec<u8>>();
		let mut rx_data = Vec::new();
		stream.write(&data).unwrap();

		// Check we get data, in pieces of arbitrary length
		while rx_data.len() < data.len() {
			match test_rx.recv() {
				::Message::Indication(::Indication::Socket(Indication::Received(ref x))) => {
					assert_eq!(x.handle, conn_handle);
					rx_data.append(&mut x.data.clone());
					socket_thread.send_nonrequest(RspReceived { handle: x.handle });
				}
				_ => panic!("Bad match"),
			};
		}

		assert_eq!(rx_data, data);

		stream.shutdown(net::Shutdown::Both).unwrap();

		// Check we get an IndDropped
		match test_rx.recv() {
			::Message::Indication(::Indication::Socket(Indication::Dropped(ref x))) => {
				assert_eq!(x.handle, conn_handle);
			}
			_ => panic!("Bad match"),
		};
	}

	#[test]
	/// Uses 127.0.1.1:8005 to receive some random data
	fn receive_data() {
		let socket_thread = make_task();
		let (reply_to, test_rx) = ::make_channel();

		let bind_req = ReqBind {
			addr: "127.0.1.1:8005".parse().unwrap(),
			context: 5678,
		};
		socket_thread.send_request(bind_req, &reply_to);
		let listen_handle = match test_rx.recv() {
			::Message::Confirmation(::Confirmation::Socket(Confirmation::Bind(ref x))) => {
				assert_eq!(x.context, 5678);
				x.result.unwrap()
			}
			_ => panic!("Bad match"),
		};

		// Make a TCP connection
		let mut stream = net::TcpStream::connect("127.0.1.1:8005").unwrap();

		// Check we get an IndConnected
		let conn_handle = match test_rx.recv() {
			::Message::Indication(::Indication::Socket(Indication::Connected(ref x))) => {
				assert_eq!(x.listen_handle, listen_handle);
				assert_eq!(x.peer, stream.local_addr().unwrap());
				x.conn_handle
			}
			_ => panic!("Bad match"),
		};

		test_rx.check_empty();

		// Send some data using the socket thread
		let data = rand::thread_rng().gen_iter().take(1024).collect::<Vec<u8>>();
		socket_thread.send_request(ReqSend {
			                           handle: conn_handle,
			                           context: 1234,
			                           data: data.clone(),
		                           },
		                           &reply_to);

		test_rx.check_empty();

		// Read all the data
		let mut rx_data = Vec::new();
		while rx_data.len() != data.len() {
			let mut part = [0u8; 16];
			if stream.read(&mut part).unwrap() == 0 {
				// We should never read zero - there should be enough
				// data in the buffer
				panic!("Zero read");
			}
			rx_data.extend_from_slice(&part);
		}

		assert_eq!(rx_data, data);

		// Check we get cfm
		match test_rx.recv() {
			::Message::Confirmation(::Confirmation::Socket(Confirmation::Send(ref x))) => {
				assert_eq!(x.handle, conn_handle);
				if let Ok(len) = x.result {
					assert_eq!(len, data.len())
				} else {
					panic!("Didn't get OK in CfmSend");
				}
				assert_eq!(x.context, 1234);
			}
			_ => panic!("Bad match"),
		};

		stream.shutdown(net::Shutdown::Both).unwrap();

		// Check we get an IndDropped
		match test_rx.recv() {
			::Message::Indication(::Indication::Socket(Indication::Dropped(ref x))) => {
				assert_eq!(x.handle, conn_handle);
			}
			_ => panic!("Bad match"),
		};

		test_rx.check_empty();

	}
}

/// Don't log the contents of the vector
impl fmt::Debug for IndReceived {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f,
		       "IndReceived {{ handle: {}, data.len: {} }}",
		       self.handle,
		       self.data.len())
	}
}

/// Don't log the contents of the vector
impl fmt::Debug for ReqSend {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f,
		       "ReqSend {{ handle: {}, data.len: {} }}",
		       self.handle,
		       self.data.len())
	}
}

/// Wrap io::Errors into SocketErrors easily
impl From<io::Error> for SocketError {
	fn from(e: io::Error) -> SocketError {
		SocketError::IOError(e.kind())
	}
}

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
