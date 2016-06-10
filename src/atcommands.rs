#![allow(dead_code)]

/// Ask the AT system to send a command
#[derive(Debug)]
pub enum ATRequest {
    SendCommand(ATCommand),
    Ping,
}

/// Response from the AT system about the command sent
#[derive(Debug)]
pub enum ATConfirmation {
    SendCommand(ATResult),
    Ping,
}

/// AT system saw something interesting
#[derive(Debug)]
pub enum ATIndication {
    Ring,
    // Optional baud rate
    Connect(Option<u32>),
    Busy,
}

/// What came back in response to a sent command
#[derive(Debug)]
pub enum ATResult {
    Ok,
    Pending,
    Connect,
    NoCarrier,
    Unknown,
}

/// Commands you can send
#[derive(Debug)]
pub enum ATCommand {
    // AT
    Empty,
    // ATD<string>
    Dial(String),
    // ATH
    HangUp,
}

pub type SocketHandle = u32;

#[derive(Debug)]
pub enum SocketError {
    BadAddress,
    Timeout,
    Unknown,
}

// Socket Messages
#[derive(Debug)]
pub enum SocketRequest {
    // Open a socket
    Open {
        addr: String,
        port: u16,
    },
    // Close an open socket
    CloseSkt(SocketHandle),
    // Send something on a socket
    Send {
        handle: SocketHandle,
        msg: String,
    },
}

#[derive(Debug)]
pub enum SocketConfirmation {
    Open(Result<SocketHandle, SocketError>),
    CloseSkt {
        handle: SocketHandle,
        result: Result<(), SocketError>,
    },
    Send {
        handle: SocketHandle,
        result: Result<(), SocketError>,
    },
}

#[derive(Debug)]
pub enum SocketIndication {
    // No more socket
    Dropped(SocketHandle),
    // Data arrived
    Received(String),
}

#[cfg(test)]
mod test {
    #[test]
    fn it_works() {}
}
