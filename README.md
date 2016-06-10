# cuslip
## copper slip makes threads go smoothly when you have Rust

Cuslip is designed to encourage a message-passing based approach to stack development.

In a stack based system, consider two layers, `N` and `N+1`. Layer `N` is the *provider* of a *service* and `N+1` is the *user* of that *service*. *Users* and *providers* exist in different threads and speak to each other using *primitives*. There are four sorts of *primitive*.

1. Request (or REQ). A *user* sends a *request* to a *provider* to request that it do something. Examples might include, open a new connection, or transmit some data on an existing connection.
2. Confirmation (or CFM). A *provider* sends a *confirmation* back to a *user*. Each *request* solicits exactly one *confirmation* in reply - it usually tells the *user* whether the *request* was successful or not and includes any relevant results from the *request*.
3. Indication (or IND). Sometimes a *provider* may wish to indicate to the *user* than an event has occured - this is an *indication*. For example, a *provider* may wish to *indicate* that data has arrived on a connection, or that a connection is no longer in existence.
4. Response (or RSP). Rarely, it is necessary for a *user* to respond to an *indication* in such a fashion that it would not make sense to use a *request*/*confirmation* pair. In this case, the *user* may send a *response* to a service.

```
+-------------------------+
|     Layer N + 1         |
+-------------------------+
     |    ^     |     ^
  REQ| CFM|  IND|  RSP|
     |    |     |     |
     v    |     v     |
+-------------------------+
|     Layer N             |
+-------------------------+
```

Note that in all four cases, the primitives are defined by the *provider* and the definitions are used by the *user*.

In some cases, the *confirmation* for a *request* is fast - user requests provider sends some data, confirmation provider has received data for sending, time passes, provider indicates that sending is now complete. In other cases, the *confirmation* for a *request* is slow - user requests provider sends some data, time passes, provider confirms that data has now been sent. Deciding between these two models is something of an artform.

Often it is useful to describe these interaction via the medium of the Message Sequence Diagram.

```
    Layer N +1         Layer N
       |                 |
       |      REQ        |
       |---------------->|
       |                 |
       |      CFM        |
       |<----------------|
       |                 |
       |      IND        |
       |<----------------|
       |                 |
       |      RSP        |
       |---------------->|
       |                 |
```

During very early development, the code is built as a socket based AT command engine. It will open a socket to the specified address and port and it will repeatedly attempt to dial out. The layers are:

1. Application. All the application logic - the top level module. It is a *user*, but never a *provider*.
2. ProtoServer. Decodes a custom protocol sent over a socket.
3. SocketServer. Handles TCP sockets.