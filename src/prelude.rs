//! # cuslip - Making threads easier when you have rust.
//!
//! Contains traits that users of cuslip should probably have in scope.
//!
//! To use cuslip, make sure your program/crate has:
//!
//! ```
//! use cuslip::prelude::*;
//! use cuslip;
//! ```

// ****************************************************************************
//
// Imports
//
// ****************************************************************************

use ::{Message, MessageSender};

// ****************************************************************************
//
// Public Types
//
// ****************************************************************************

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
