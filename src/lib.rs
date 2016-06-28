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
pub enum Message {
    Request(MessageSender, MessageRequest),
    Confirmation(MessageConfirmation),
    Indication(MessageIndication),
}

/// The set of all requests in the system.
/// This is an enumeration of all the interfaces.
pub enum MessageRequest {
    Socket(socket::SocketReq),
}

/// The set of all confirmations in the system.
/// This is an enumeration of all the interfaces.
pub enum MessageConfirmation {
    Socket(socket::SocketCfm),
}

/// The set of all indications in the system.
/// This is an enumeration of all the interfaces.
pub enum MessageIndication {
    Socket(socket::SocketInd),
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
