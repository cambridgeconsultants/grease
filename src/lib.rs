//! # cuslip - Making threads easier when you have rust.

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
/// ready for sending down a MessageSender channel endpoint.
pub trait NonRequestSendable {
    fn wrap(self) -> Message;
}

/// Implementors of the RequestSendable trait can be easily wrapped in a message
/// ready for sending down a MessageSender channel endpoint.
pub trait RequestSendable {
    fn wrap(self, reply_to: MessageSender) -> Message;
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
