//! # socket - A TCP/UDP socket server task
//!
//! The grease socket task makes handling sockets easier. A user requests the
//! socket task opens a new socket and it does so (if possible). The user then
//! receives asynchronous indications when data arrives on the socket and/or
//! when the socket closes.

extern crate grease;
#[macro_use]
extern crate log;
extern crate mio;
extern crate mio_more;

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

use grease::Context;

// ****************************************************************************
//
// Public Messages
//
// ****************************************************************************

/// Requests that can be sent to the Socket task
#[derive(Debug)]
pub enum Request {
	/// A Bind Request - Bind a listen socket
	Bind(ReqBind),
	/// A Close request - Close an open connection
	Close(ReqClose),
	/// A Send request - Send something on a connection
	Send(ReqSend),
}

/// Confirms sent from the Socket task in answer to a Request
#[derive(Debug)]
pub enum Confirm {
	/// A Bind Confirm - Bound a listen socket
	Bind(CfmBind),
	/// A Close Confirm - Closed an open connection
	Close(CfmClose),
	/// A Send Confirm - Sent something on a connection
	Send(CfmSend),
}

/// Asynchronous indications sent by the Socket task.
/// TODO: Perhaps indicate EOF/HUP ind here? As distinct from dropping the
/// connection handle (which would mean you couldn't write either).
#[derive(Debug)]
pub enum Indication {
	/// A Connected Indication - Indicates that a listening socket has been
	/// connected to
	Connected(IndConnected),
	/// A Dropped Indication - Indicates that an open socket has been dropped
	Dropped(IndDropped),
	/// A Received Indication - Indicates that data has arrived on an open
	/// socket
	Received(IndReceived),
}

/// Responses to Indications required
#[derive(Debug)]
pub enum Response {
	/// a Receieved Response - unblocks the open socket so more IndReceived can
	/// be sent
	Received(RspReceived),
}

/// Bind a listen socket
#[derive(Debug)]
pub struct ReqBind {
	/// The address to bind to
	pub addr: net::SocketAddr,
	/// Reflected in the cfm
	pub context: Context,
	/// Type of connection to bind
	pub conn_type: ConnectionType,
}

/// Close an open connection
#[derive(Debug)]
pub struct ReqClose {
	/// The handle from a IndConnected
	pub handle: ConnHandle,
	/// Reflected in the cfm
	pub context: Context,
}

/// Send something on a connection
pub struct ReqSend {
	/// The handle from a CfmBind
	pub handle: ConnHandle,
	/// Some (maybe) unique identifier
	pub context: Context,
	/// The data to be sent
	pub data: Vec<u8>,
}

/// Reply to a ReqBind.
#[derive(Debug)]
pub struct CfmBind {
	/// Either a new ListenHandle or an error
	pub result: Result<ListenHandle, SocketError>,
	/// Reflected from the req
	pub context: Context,
}

/// Reply to a ReqClose. Will flush out all
/// existing data.
#[derive(Debug)]
pub struct CfmClose {
	/// The handle requested for closing
	pub handle: ConnHandle,
	/// Success or failed
	pub result: Result<(), SocketError>,
	/// Reflected from the req
	pub context: Context,
}

/// Reply to a ReqSend. The data has not necessarily
/// been sent, but it is safe to send some more data.
#[derive(Debug)]
pub struct CfmSend {
	/// The handle requested for sending
	pub handle: ConnHandle,
	/// Amount sent or error
	pub result: Result<usize, SocketError>,
	/// Some (maybe) unique identifier
	pub context: Context,
}

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

/// Indicates that a socket has been dropped.
#[derive(Debug)]
pub struct IndDropped {
	/// The handle that is no longer valid
	pub handle: ConnHandle,
}

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

/// Tell the task that more data can now be sent.
#[derive(Debug)]
pub struct RspReceived {
	/// Which handle is now free to send up more data
	pub handle: ConnHandle,
}

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

/// Users can use this to send us messages.
pub type ServiceProviderHandle =
	grease::ServiceProviderHandle<Request, Confirm, Indication, Response>;

/// A `socket` specific wrapper around `grease::ServiceUserHandle`. We use this to
/// talk to our users.
pub type ServiceUserHandle = grease::ServiceUserHandle<Confirm, Indication>;

/// Represents something a socket service user can hold on to to send us
/// message.
pub struct Handle {
	chan: mio_more::channel::Sender<Incoming>,
}

/// Uniquely identifies an listening socket
pub type ListenHandle = Context;

/// Uniquely identifies an open socket
pub type ConnHandle = Context;

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

/// The sort of connections we can make.
#[derive(Debug, Copy, Clone)]
pub enum ConnectionType {
	/// Stream, aka a TCP connection
	Stream,
	/// Datagram, aka a UDP connection
	Datagram,
}

// ****************************************************************************
//
// Private Types
//
// ****************************************************************************

/// The set of all messages that this task can receive.
enum Incoming {
	/// One of our own requests that has come in
	Request(Request, ServiceUserHandle),
	/// One of our own responses that has come in
	Response(Response),
}

/// Created for every bound (i.e. listening) socket
struct ListenSocket {
	handle: ListenHandle,
	ind_to: ServiceUserHandle,
	listener: mio::tcp::TcpListener,
}

/// Create for every pending write
struct PendingWrite {
	context: Context,
	sent: usize,
	data: Vec<u8>,
	reply_to: ServiceUserHandle,
}

/// Created for every connection receieved on a ListenSocket
struct ConnectedSocket {
	// parent: ListenHandle,
	ind_to: ServiceUserHandle,
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
	next_handle: Context,
	/// The special channel our messages arrive on
	mio_rx: mio_more::channel::Receiver<Incoming>,
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

const MAX_READ_LEN: usize = 2048;
const MESSAGE_TOKEN: mio::Token = mio::Token(0);

// ****************************************************************************
//
// Public Functions
//
// ****************************************************************************

/// Creates a new socket task. Returns an object that can be used
/// to send this task messages.
pub fn make_task() -> ServiceProviderHandle {
	let (mio_tx, mio_rx) = mio_more::channel::channel();
	thread::spawn(move || {
		let mut task_context = TaskContext::new(mio_rx);
		loop {
			task_context.poll();
		}
	});
	Box::new(Handle { chan: mio_tx })
}

// ****************************************************************************
//
// Private Functions
//
// ****************************************************************************

impl TaskContext {
	fn poll(&mut self) {
		let mut events = mio::Events::with_capacity(1024);
		let num_events = self.poll.poll(&mut events, None).unwrap();
		trace!("Woke up! Handling num_events={}", num_events);
		for event in events.iter() {
			self.process_event(event)
		}
	}

	/// Called when mio has an update on a registered listener or connection
	/// We have to check the Ready to find out whether our socket is
	/// readable or writable
	fn process_event(&mut self, event: mio::Event) {
		let ready = event.readiness();
		let token = event.token();
		debug!("ready: {:?}, token: {:?}", ready, token);
		let handle = Context::new(token.0);
		if ready.is_readable() {
			if token == MESSAGE_TOKEN {
				loop {
					// Empty the whole message queue
					if let Ok(msg) = self.mio_rx.try_recv() {
						self.handle_message(msg);
					} else {
						break;
					}
				}
			} else if self.listeners.contains_key(&handle) {
				debug!("Readable listen socket {}?", handle);
				self.accept_new_connection(handle)
			} else if self.connections.contains_key(&handle) {
				debug!("Readable connected socket {}", handle);
				self.read_from_socket(handle)
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
				// Probably just closed
				debug!("Writable on unknown token {}", handle);
			}
		}
		// We used to check .is_error() and .is_hup() here, but they are now
		// considered UNIX only. We could put them back in, but would need to
		// handle Windows differently.
	}

	/// Called when our task has received a Message
	fn handle_message(&mut self, msg: Incoming) {
		match msg {
			// We only handle our own requests and responses
			Incoming::Request(msg, reply_to) => {
				debug!("Rx: {:?}", msg);
				self.handle_socket_req(msg, reply_to)
			}
			Incoming::Response(msg) => {
				debug!("Rx: {:?}", msg);
				self.handle_socket_rsp(msg)
			}
		}
	}

	/// Init the context
	pub fn new(mio_rx: mio_more::channel::Receiver<Incoming>) -> TaskContext {
		let t = TaskContext {
			listeners: HashMap::new(),
			connections: HashMap::new(),
			next_handle: Context::new(MESSAGE_TOKEN.0 + 1),
			mio_rx: mio_rx,
			poll: mio::Poll::new().unwrap(),
		};
		t.poll
			.register(
				&t.mio_rx,
				MESSAGE_TOKEN,
				mio::Ready::readable(),
				mio::PollOpt::level(),
			)
			.unwrap();
		t
	}

	/// Accept a new incoming connection and let the user know
	/// with a IndConnected
	fn accept_new_connection(&mut self, ls_handle: ListenHandle) {
		// We know this exists because we checked it before we got here
		let ls = self.listeners.get(&ls_handle).unwrap();
		if let Ok((stream, conn_addr)) = ls.listener.accept() {
			let cs = ConnectedSocket {
				// parent: ls.handle,
				handle: self.next_handle.take(),
				ind_to: ls.ind_to.clone(),
				connection: stream,
				outstanding: false,
				pending_writes: VecDeque::new(),
			};
			self.poll
				.register(
					&cs.connection,
					mio::Token(cs.handle.as_usize()),
					mio::Ready::readable() | mio::Ready::writable(),
					mio::PollOpt::edge(),
				)
				.unwrap();
			let ind = IndConnected {
				listen_handle: ls.handle,
				conn_handle: cs.handle,
				peer: conn_addr,
			};
			self.connections.insert(cs.handle, cs);
			ls.ind_to.send_indication(Indication::Connected(ind));
		} else {
			warn!("accept returned None!");
		}
	}

	/// Data can be sent a connected socket. Send what we have
	fn pending_writes(&mut self, cs_handle: ConnHandle) {
		// We know this exists because we checked it before we got here
		let cs = self.connections.get_mut(&cs_handle).unwrap();
		loop {
			if let Some(mut pw) = cs.pending_writes.pop_front() {
				let to_send = pw.data.len() - pw.sent;
				match cs.connection.write(&pw.data[pw.sent..]) {
					Ok(len) if len < to_send => {
						let left = to_send - len;
						debug!(
							"Sent {} of {} pending, leaving {} on handle: {}",
							len, to_send, left, cs.handle
						);
						pw.sent = pw.sent + len;
						cs.pending_writes.push_front(pw);
						// No cfm here - we wait some more
						break;
					}
					Ok(_) => {
						debug!("Sent all {} pending on handle: {}", to_send, cs.handle);
						pw.sent = pw.sent + to_send;
						let cfm = CfmSend {
							handle: cs.handle,
							context: pw.context,
							result: Ok(pw.sent),
						};
						pw.reply_to.send_confirm(Confirm::Send(cfm));
					}
					Err(err) => {
						warn!(
							"Send error on handle: {} (pending), err: {}",
							cs.handle, err
						);
						let cfm = CfmSend {
							handle: cs.handle,
							context: pw.context,
							result: Err(err.into()),
						};
						pw.reply_to.send_confirm(Confirm::Send(cfm));
						break;
					}
				}
			} else {
				break;
			}
		}
	}

	/// Data is available on a connected socket. Pass it up
	fn read_from_socket(&mut self, cs_handle: ConnHandle) {
		debug!("Reading connection {}", cs_handle);
		let mut need_close = false;
		{
			// We know this exists because we checked it before we got here
			let cs = self.connections.get_mut(&cs_handle).unwrap();
			// Only pass up one indication at a time
			if !cs.outstanding {
				// Cap the max amount we will read
				let mut buffer = vec![0u8; MAX_READ_LEN];
				match cs.connection.read(buffer.as_mut_slice()) {
					Ok(0) => {
						debug!("Read nothing on handle: {}", cs_handle);
						// Reading zero bytes after a POLLIN means connection is closed
						// See http://www.greenend.org.uk/rjk/tech/poll.html
						need_close = true;
					}
					Ok(len) => {
						debug!("Read {} octets on handle: {}", len, cs_handle);
						let _ = buffer.split_off(len);
						let ind = IndReceived {
							handle: cs.handle,
							data: buffer,
						};
						cs.outstanding = true;
						cs.ind_to.send_indication(Indication::Received(ind));
					}
					Err(ref err) if err.kind() == ::std::io::ErrorKind::WouldBlock => {}
					Err(err) => {
						warn!("Read error on handle: {}, err: {}", cs.handle, err);
						need_close = true;
					}
				}
			} else {
				debug!("Not reading - outstanding ind on handle: {}", cs_handle)
			}
		}
		if need_close {
			self.dropped(cs_handle);
		}
	}

	/// Connection has gone away. Clean up.
	fn dropped(&mut self, cs_handle: ConnHandle) {
		// We know this exists because we checked it before we got here
		let cs = self.connections.remove(&cs_handle).unwrap();
		self.poll.deregister(&cs.connection).unwrap();
		let ind = IndDropped { handle: cs_handle };
		cs.ind_to.send_indication(Indication::Dropped(ind));
	}

	/// Handle requests
	pub fn handle_socket_req(&mut self, req: Request, reply_to: ServiceUserHandle) {
		match req {
			Request::Bind(x) => self.handle_bind(x, reply_to),
			Request::Close(x) => self.handle_close(x, reply_to),
			Request::Send(x) => self.handle_send(x, reply_to),
		}
	}

	/// Open a new socket with the given parameters.
	fn handle_bind(&mut self, req_bind: ReqBind, reply_to: ServiceUserHandle) {
		info!("Binding {:?} on {}...", req_bind.conn_type, req_bind.addr);
		match req_bind.conn_type {
			ConnectionType::Stream => self.handle_stream_bind(req_bind, reply_to),
			ConnectionType::Datagram => panic!("Datagrams not supported yet"),
		}
	}

	fn handle_stream_bind(&mut self, req_bind: ReqBind, reply_to: ServiceUserHandle) {
		let cfm = match mio::tcp::TcpListener::bind(&req_bind.addr) {
			Ok(server) => {
				let h = self.next_handle.take();
				debug!("Allocated listen handle: {}", h);
				let l = ListenSocket {
					handle: h,
					// We assume any future indications should be sent
					// to the same place we send the CfmBind.
					ind_to: reply_to.clone(),
					listener: server,
				};
				match self.poll.register(
					&l.listener,
					mio::Token(h.as_usize()),
					mio::Ready::readable(),
					mio::PollOpt::level(),
				) {
					Ok(_) => {
						self.listeners.insert(h, l);
						CfmBind {
							result: Ok(h),
							context: req_bind.context,
						}
					}
					Err(io_error) => CfmBind {
						result: Err(io_error.into()),
						context: req_bind.context,
					},
				}
			}
			Err(io_error) => CfmBind {
				result: Err(io_error.into()),
				context: req_bind.context,
			},
		};
		reply_to.send_confirm(Confirm::Bind(cfm));
	}

	/// Handle a ReqClose
	fn handle_close(&mut self, req_close: ReqClose, reply_to: ServiceUserHandle) {
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
		reply_to.send_confirm(Confirm::Close(cfm));
	}

	/// Handle a ReqSend
	fn handle_send(&mut self, req_send: ReqSend, reply_to: ServiceUserHandle) {
		if let Some(cs) = self.connections.get_mut(&req_send.handle) {
			let to_send = req_send.data.len();
			// Let's see how much we can get rid off right now
			if cs.pending_writes.len() > 0 {
				debug!(
					"Storing write len {} on handle: {}",
					to_send, req_send.handle
				);
				let pw = PendingWrite {
					sent: 0,
					context: req_send.context,
					data: req_send.data,
					reply_to: reply_to,
				};
				cs.pending_writes.push_back(pw);
			// No cfm here - we wait
			} else {
				match cs.connection.write(&req_send.data) {
					Ok(len) if len < to_send => {
						let left = to_send - len;
						debug!(
							"Sent {} of {}, leaving {} on handle: {}",
							len, to_send, left, cs.handle
						);
						let pw = PendingWrite {
							sent: len,
							context: req_send.context,
							data: req_send.data,
							reply_to: reply_to,
						};
						cs.pending_writes.push_back(pw);
						// No cfm here - we wait
					}
					Ok(_) => {
						debug!("Sent all {} on handle: {}", to_send, cs.handle);
						let cfm = CfmSend {
							context: req_send.context,
							handle: req_send.handle,
							result: Ok(to_send),
						};
						reply_to.send_confirm(Confirm::Send(cfm));
					}
					Err(err) => {
						warn!("Send error on handle: {}, err: {}", cs.handle, err);
						let cfm = CfmSend {
							context: req_send.context,
							handle: req_send.handle,
							result: Err(err.into()),
						};
						reply_to.send_confirm(Confirm::Send(cfm));
					}
				}
			}
		} else {
			let cfm = CfmSend {
				result: Err(SocketError::BadHandle),
				context: req_send.context,
				handle: req_send.handle,
			};
			reply_to.send_confirm(Confirm::Send(cfm));
		}
	}

	/// Handle responses
	pub fn handle_socket_rsp(&mut self, rsp: Response) {
		match rsp {
			Response::Received(x) => self.handle_received(x),
		}
	}

	/// Someone wants more data
	fn handle_received(&mut self, rsp_received: RspReceived) {
		let mut need_read = false;
		// Read response handle might not be valid - it might
		// have crossed over with a disconnect.
		if let Some(cs) = self.connections.get_mut(&rsp_received.handle) {
			cs.outstanding = false;
			// Let's try and send them some more data - if it exhausts the
			// buffer on the socket, the event loop will automatically set
			// itself to interrupt when more data arrives
			need_read = true;
		}
		if need_read {
			// Try and read it - won't hurt if we can't.
			self.read_from_socket(rsp_received.handle)
		}
	}
}

impl grease::ServiceProvider<Request, Confirm, Indication, Response> for Handle {
	fn send_request(&self, req: Request, reply_to: &grease::ServiceUser<Confirm, Indication>) {
		self.chan
			.send(Incoming::Request(req, reply_to.clone()))
			.unwrap();
	}

	fn send_response(&self, rsp: Response) {
		self.chan.send(Incoming::Response(rsp)).unwrap();
	}

	fn clone(&self) -> ServiceProviderHandle {
		Box::new(Handle {
			chan: self.chan.clone(),
		})
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
			pw.reply_to.send_confirm(Confirm::Send(cfm));
		}
	}
}

// #[cfg(test)]
// mod test {
// 	use std::io::prelude::*;
// 	use std::net;
// 	use rand;
// 	use rand::Rng;
// 	use env_logger;
// 	use super::*;

// 	#[test]
// 	/// Binds 127.0.1.1:8000
// 	fn bind_port_ok() {
// 		let socket_thread = make_task();
// 		let (reply_to, test_rx) = mpsc::channel();
// 		let bind_req = ReqBind {
// 			addr: "127.0.1.1:8000".parse().unwrap(),
// 			context: Context(1234),
// 			conn_type: ConnectionType::Stream,
// 		};
// 		socket_thread.send_request(bind_req, &reply_to);
// 		let cfm = test_rx.recv();
// 		match cfm {
// 			Message::Confirm(ConfirmTask::Socket(Confirm::Bind(ref x))) => {
// 				assert_eq!(x.context, Context(1234));
// 				assert!(x.result.is_ok());
// 			}
// 			_ => panic!("Bad match"),
// 		}
// 	}

// 	#[test]
// 	/// Fails to bind 127.0.1.1:22 (because you need to be root)
// 	fn bind_port_fail() {
// 		let socket_thread = make_task();
// 		let (reply_to, test_rx) = ::make_channel();
// 		// Shouldn't be able to bind :22 as normal user
// 		let bind_req = ReqBind {
// 			addr: "127.0.1.1:22".parse().unwrap(),
// 			context: Context(5678),
// 			conn_type: ConnectionType::Stream,
// 		};
// 		socket_thread.send_request(bind_req, &reply_to);
// 		let cfm = test_rx.recv();
// 		match cfm {
// 			Message::Confirm(ConfirmTask::Socket(Confirm::Bind(ref x))) => {
// 				assert_eq!(x.context, Context(5678));
// 				assert!(x.result.is_err());
// 			}
// 			_ => panic!("Bad match"),
// 		}
// 	}

// 	#[test]
// 	/// Fails to bind 8.8.8.8:8000 (because you don't have that i/f)
// 	fn bind_if_fail() {
// 		let socket_thread = make_task();
// 		let (reply_to, test_rx) = ::make_channel();
// 		// Shouldn't be able to bind :22 as normal user
// 		let bind_req = ReqBind {
// 			addr: "8.8.8.8:8000".parse().unwrap(),
// 			context: Context(6666),
// 			conn_type: ConnectionType::Stream,
// 		};
// 		socket_thread.send_request(bind_req, &reply_to);
// 		let cfm = test_rx.recv();
// 		match cfm {
// 			Message::Confirm(ConfirmTask::Socket(Confirm::Bind(ref x))) => {
// 				assert_eq!(x.context, Context(6666));
// 				assert!(x.result.is_err());
// 			}
// 			_ => panic!("Bad match"),
// 		}
// 	}

// 	#[test]
// 	/// Connects to a socket on 127.0.1.1:8001
// 	fn connect() {
// 		let socket_thread = make_task();
// 		let (reply_to, test_rx) = ::make_channel();

// 		let bind_req = ReqBind {
// 			addr: "127.0.1.1:8001".parse().unwrap(),
// 			context: Context(5678),
// 			conn_type: ConnectionType::Stream,
// 		};
// 		socket_thread.send_request(bind_req, &reply_to);
// 		let listen_handle = match test_rx.recv() {
// 			Message::Confirm(ConfirmTask::Socket(Confirm::Bind(ref x))) => {
// 				assert_eq!(x.context, Context(5678));
// 				x.result.unwrap()
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		// Make a TCP connection
// 		let stream = net::TcpStream::connect("127.0.1.1:8001").unwrap();

// 		// Check we get an IndConnected
// 		let conn_handle = match test_rx.recv() {
// 			Message::Indication(IndicationTask::Socket(Indication::Connected(ref x))) => {
// 				assert_eq!(x.listen_handle, listen_handle);
// 				assert_eq!(x.peer, stream.local_addr().unwrap());
// 				x.conn_handle
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		stream.shutdown(net::Shutdown::Both).unwrap();

// 		// Check we get an IndDropped
// 		match test_rx.recv() {
// 			Message::Indication(IndicationTask::Socket(Indication::Dropped(ref x))) => {
// 				assert_eq!(x.handle, conn_handle);
// 			}
// 			_ => panic!("Bad match"),
// 		};
// 	}

// 	#[test]
// 	/// Uses 127.0.1.1:8002 and 127.0.1.1:8003 to send random data
// 	fn two_connections() {
// 		let socket_thread = make_task();
// 		let (reply_to, test_rx) = ::make_channel();

// 		let bind_req = ReqBind {
// 			addr: "127.0.1.1:8002".parse().unwrap(),
// 			context: Context(5678),
// 			conn_type: ConnectionType::Stream,
// 		};
// 		socket_thread.send_request(bind_req, &reply_to);
// 		let listen_handle8002 = match test_rx.recv() {
// 			Message::Confirm(ConfirmTask::Socket(Confirm::Bind(ref x))) => {
// 				assert_eq!(x.context, Context(5678));
// 				x.result.unwrap()
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		let bind_req = ReqBind {
// 			addr: "127.0.1.1:8003".parse().unwrap(),
// 			context: Context(5678),
// 			conn_type: ConnectionType::Stream,
// 		};
// 		socket_thread.send_request(bind_req, &reply_to);
// 		let listen_handle8003 = match test_rx.recv() {
// 			Message::Confirm(ConfirmTask::Socket(Confirm::Bind(ref x))) => {
// 				assert_eq!(x.context, Context(5678));
// 				x.result.unwrap()
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		// Make two TCP connections
// 		let stream8003 = net::TcpStream::connect("127.0.1.1:8003").unwrap();
// 		let stream8002 = net::TcpStream::connect("127.0.1.1:8002").unwrap();

// 		// Check we get an IndConnected, twice
// 		let conn_handle8003 = match test_rx.recv() {
// 			Message::Indication(IndicationTask::Socket(Indication::Connected(ref x))) => {
// 				assert_eq!(x.listen_handle, listen_handle8003);
// 				assert_eq!(x.peer, stream8003.local_addr().unwrap());
// 				x.conn_handle
// 			}
// 			_ => panic!("Bad match"),
// 		};
// 		let conn_handle8002 = match test_rx.recv() {
// 			Message::Indication(IndicationTask::Socket(Indication::Connected(ref x))) => {
// 				assert_eq!(x.listen_handle, listen_handle8002);
// 				assert_eq!(x.peer, stream8002.local_addr().unwrap());
// 				x.conn_handle
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		// Shutdown the 8002 connection
// 		stream8002.shutdown(net::Shutdown::Both).unwrap();

// 		// Check we get an IndDropped
// 		match test_rx.recv() {
// 			Message::Indication(IndicationTask::Socket(Indication::Dropped(ref x))) => {
// 				assert_eq!(x.handle, conn_handle8002);
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		// Shutdown the 8003 connection
// 		stream8003.shutdown(net::Shutdown::Both).unwrap();

// 		// Check we get an IndDropped
// 		match test_rx.recv() {
// 			Message::Indication(IndicationTask::Socket(Indication::Dropped(ref x))) => {
// 				assert_eq!(x.handle, conn_handle8003);
// 			}
// 			_ => panic!("Bad match"),
// 		};
// 	}

// 	#[test]
// 	/// Uses 127.0.1.1:8004 to send random data
// 	/// With the changes to remove hup and error, this test no longer passes
// 	/// 100%. I think that's because there's a race between closing the socket
// 	/// and sending the read response. We do a read, and get data. That was
// 	/// actually the end of the data, but we didn't know it. When we do
// 	/// a read after the response has arrived, we consider it speculative
// 	/// and so don't declare the lack of data as an EOF marker.
// 	fn send_data() {
// 		let _ = env_logger::init();

// 		let socket_thread = make_task();
// 		let (reply_to, test_rx) = ::make_channel();

// 		let bind_req = ReqBind {
// 			addr: "127.0.1.1:8004".parse().unwrap(),
// 			context: Context(5678),
// 			conn_type: ConnectionType::Stream,
// 		};
// 		socket_thread.send_request(bind_req, &reply_to);
// 		let listen_handle = match test_rx.recv() {
// 			Message::Confirm(ConfirmTask::Socket(Confirm::Bind(ref x))) => {
// 				assert_eq!(x.context, Context(5678));
// 				x.result.unwrap()
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		// Make a TCP connection
// 		let mut stream = net::TcpStream::connect("127.0.1.1:8004").unwrap();

// 		// Check we get an IndConnected
// 		let conn_handle = match test_rx.recv() {
// 			Message::Indication(IndicationTask::Socket(Indication::Connected(ref x))) => {
// 				assert_eq!(x.listen_handle, listen_handle);
// 				assert_eq!(x.peer, stream.local_addr().unwrap());
// 				x.conn_handle
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		// Send some data
// 		let data = rand::thread_rng()
// 			.gen_iter()
// 			.take(1024)
// 			.collect::<Vec<u8>>();
// 		let mut rx_data = Vec::new();
// 		stream.write(&data).unwrap();

// 		// Check we get data, in pieces of arbitrary length
// 		while rx_data.len() < data.len() {
// 			match test_rx.recv() {
// 				Message::Indication(IndicationTask::Socket(Indication::Received(ref x))) => {
// 					assert_eq!(x.handle, conn_handle);
// 					rx_data.append(&mut x.data.clone());
// 					socket_thread.send_nonrequest(RspReceived { handle: x.handle });
// 				}
// 				_ => panic!("Bad match"),
// 			};
// 		}

// 		assert_eq!(rx_data, data);

// 		stream.shutdown(net::Shutdown::Both).unwrap();

// 		drop(stream);

// 		// Check we get an IndDropped
// 		match test_rx.recv() {
// 			Message::Indication(IndicationTask::Socket(Indication::Dropped(ref x))) => {
// 				assert_eq!(x.handle, conn_handle);
// 			}
// 			_ => panic!("Bad match"),
// 		};
// 	}

// 	#[test]
// 	/// Uses 127.0.1.1:8005 to receive some random data
// 	fn receive_data() {
// 		let socket_thread = make_task();
// 		let (reply_to, test_rx) = ::make_channel();

// 		let bind_req = ReqBind {
// 			addr: "127.0.1.1:8005".parse().unwrap(),
// 			context: Context(5678),
// 			conn_type: ConnectionType::Stream,
// 		};
// 		socket_thread.send_request(bind_req, &reply_to);
// 		let listen_handle = match test_rx.recv() {
// 			Message::Confirm(ConfirmTask::Socket(Confirm::Bind(ref x))) => {
// 				assert_eq!(x.context, Context(5678));
// 				x.result.unwrap()
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		// Make a TCP connection
// 		let mut stream = net::TcpStream::connect("127.0.1.1:8005").unwrap();

// 		// Check we get an IndConnected
// 		let conn_handle = match test_rx.recv() {
// 			Message::Indication(IndicationTask::Socket(Indication::Connected(ref x))) => {
// 				assert_eq!(x.listen_handle, listen_handle);
// 				assert_eq!(x.peer, stream.local_addr().unwrap());
// 				x.conn_handle
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		test_rx.check_empty();

// 		// Send some data using the socket thread
// 		let data = rand::thread_rng()
// 			.gen_iter()
// 			.take(1024)
// 			.collect::<Vec<u8>>();
// 		socket_thread.send_request(
// 			ReqSend {
// 				handle: conn_handle,
// 				context: Context(1234),
// 				data: data.clone(),
// 			},
// 			&reply_to,
// 		);

// 		test_rx.check_empty();

// 		// Read all the data
// 		let mut rx_data = Vec::new();
// 		while rx_data.len() != data.len() {
// 			let mut part = [0u8; 16];
// 			if stream.read(&mut part).unwrap() == 0 {
// 				// We should never read zero - there should be enough
// 				// data in the buffer
// 				panic!("Zero read");
// 			}
// 			rx_data.extend_from_slice(&part);
// 		}

// 		assert_eq!(rx_data, data);

// 		// Check we get cfm
// 		match test_rx.recv() {
// 			Message::Confirm(ConfirmTask::Socket(Confirm::Send(ref x))) => {
// 				assert_eq!(x.handle, conn_handle);
// 				if let Ok(len) = x.result {
// 					assert_eq!(len, data.len())
// 				} else {
// 					panic!("Didn't get OK in CfmSend");
// 				}
// 				assert_eq!(x.context, Context(1234));
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		stream.shutdown(net::Shutdown::Both).unwrap();

// 		// Check we get an IndDropped
// 		match test_rx.recv() {
// 			Message::Indication(IndicationTask::Socket(Indication::Dropped(ref x))) => {
// 				assert_eq!(x.handle, conn_handle);
// 			}
// 			_ => panic!("Bad match"),
// 		};

// 		test_rx.check_empty();
// 	}
// }

/// Don't log the contents of the vector
impl fmt::Debug for IndReceived {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(
			f,
			"IndReceived {{ handle: {}, data.len: {} }}",
			self.handle,
			self.data.len()
		)
	}
}

/// Don't log the contents of the vector
impl fmt::Debug for ReqSend {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(
			f,
			"ReqSend {{ handle: {}, data.len: {} }}",
			self.handle,
			self.data.len()
		)
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
