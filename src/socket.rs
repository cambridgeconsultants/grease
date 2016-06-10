#![allow(dead_code)]

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
