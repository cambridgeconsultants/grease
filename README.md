# grease
## Grease makes threads go smoothly when you have Rust

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

Note, the assumption is that if a *user* sends a *provider* a *request*, the *user* _will_ get a *confirmation*. To significantly simplify the code there should be no error handling for the case that one doesn't arrive. *Providers* must ensure that the *confirmation* is either fast (sent immediately in the routine that processes the *request*), or gated on a *confirmation* from a subsequent *provider* below (which has the same constraints). If the *confirmation* is gated on, for example, an *indication* from a subsequent *provider* that might never arrive, then timeouts must be used.

Often it is useful to describe these interaction via the medium of the Message Sequence Diagram.

```
    Layer N+1         Layer N
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
       | - - - - - - - ->|
       |                 |
```

Each layer in the system is implemented as a *task* and each *task* runs in its own thread. Each task has a single multiple-producer, single-consumer (mpsc) channel. Both *Indications* and *confirmations* from providers below and *requests* and *responses* from any users above are sent to this channel. The helper function grease::make_channel() makes an std::sync::mpsc::Channel of the correct type.

# Example Application

To demonstrate the system, it includes the following example tasks:

```
+---------------------------+
|        Application        |
+-------------+-------------+
| ProtoServer | Database    |
+-------------+-------------+
| Socket      |
+-------------+

```

* Application. All the application logic - the top level module. It is a *user*, but never a *provider*. It is also known as an *inflexion point* - a point where messages stop going up, turn around and start going down again. A big system may have multiple *inflexion points* but we only have one.
* ProtoServer. Decodes a custom protocol sent over a socket.
* SocketServer. Handles TCP sockets.
* Database. Stores information which Application needs to service requests from remote systems (which speak Proto).

## Proto

Proto is a noddy protocol I've invented which you can speak over any sort of stream. In this system, you could replace Socket with Serial and it would still work. Whether you can generically implement a Stream in both the Socket and Serial tasks such that ProtoServer didn't care which you used (yet where they still maintain their unique connection properties) is something I tend to explore later.

### Commands

Commands are terminated by a newline. Carriage returns are ignored. Commands and arguments are space separated. Arguments may not contain space characters or newlines or carriage returns - they must be escaped as BACKSLASH||SPACE, BACKSLASH||r and BACKSLASH||n respectively. Commands and arguments must be valid UTF-8. Binary data must be encoded as hex or base64 or somesuch.

* GET <key> -> <value>
* PUT <key> <value>
* LIST -> [<key>,...]<key>
* TIME -> <time>

### GET

Obtains some value from the database by its key.

### PUT

Stores some value in the database by its key.

### LIST

Shows all keys in the database.

### TIME

Returns the current time in UTC as an ISO 8601 string.
