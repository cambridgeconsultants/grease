# grease
## Grease makes threads go smoothly when you have Rust

Grease is designed to facilitate a message-passing based approach to stack development. Message passing between tasks, as opposed to function-call based stacks, has a number of advantages:

* Each task gets its own stack, which makes it easier to determine the maximum stack usage and hence the appropriate stack size.
* Message passing encourages an interface-first approach.
* Logging the messages as they are passed gives a low-cost but very useful mechanism for diagnosing bugs.

In a stack based system, consider two layers, `N` and `N+1`. Layer `N` is the *provider* of a *service* and `N+1` is the *user* of that *service*. *Users* and *providers* exist in different threads and speak to each other using *messages*. There are four sorts of *message*.

1. Request (or REQ). A *user* sends a *request* to a *provider* to request that it do something. Examples might include, open a new connection, or transmit some data on an existing connection.
2. Confirmation (or CFM). A *provider* sends a *confirmation* back to a *user*. Each *request* solicits exactly one *confirmation* in reply - it usually tells the *user* whether the *request* was successful or not and includes any relevant results from the *request*.
3. Indication (or IND). Sometimes a *provider* may wish to indicate to the *user* than an event has occured - this is an *indication*. For example, a *provider* may wish to *indicate* that data has arrived on a connection, or perhaps that a connection is no longer in existence.
4. Response (or RSP). Rarely, it is necessary for a *user* to respond to an *indication* in such a fashion that it would not make sense to use a *request*/*confirmation* pair. In this case, the *user* may send a *response* to a service. An example might be a task which sends up received data indications - if the received data indications were sent up with no flow control, it might be possible for the system to run out of memory. By requiring a *response* to be received for each *indication* before another *indication* can be sent, the rate of messages is limited by the receiver rather than the sender. It would be possible to use a Request in place of a Response, but the corresponding Confirmation would have no value.

```
+-------------------------+
|     Layer N + 1         |
+-------------------------+
     |    ^     ^     |
  REQ| CFM|  IND|  RSP|
     |    |     |     |
     v    |     |     V
+-------------------------+
|     Layer N             |
+-------------------------+
```

Note that in all four cases, the messages are defined by the *provider* and those message definitions are used by the *user*.

In some cases, the *confirmation* for a *request* is fast:

* A user requests that a provider sends some data, for example.
* The provider sends a confirmation to the user, showing that the provider has received that data for sending.
* Time passes.
* The provider sends an indication to the user to show that sending is now complete.

In other cases, the *confirmation* for a *request* is slow:

* A user requests that a provider sends some data, for example.
* Time passes.
* The provider sends an confirmation to the user to show that sending is now complete.

Deciding between these two models is something of an art form. The latter is simpler, but limits the ability of the user to place multiple requests simultaneously - it is considered poor form to send multiple requests without gating each request on the previous request's confirmation as if the provider is busy, you will just fill up its input queue.

Note, the assumption is that if a *user* sends a *provider* a *request*, the *user* _will_ get a *confirmation*. To significantly simplify the code there should be no error handling for the case that one doesn't arrive. *Providers* must ensure that the *confirmation* is either fast (sent immediately in the routine that processes the *request*), or gated on a *confirmation* from a subsequent *provider* below (which has the same constraints), or guaranteed in some other fashion (perhaps using a timeout). For example, if the *confirmation* is gated on an *indication* from a subsequent *provider* then as that indication might never arrive a timeout must be used to ensure the confirmation is eventually sent.

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
       |................>|
       |                 |
```

Each layer in the system is implemented as a *task* and each *task* runs in its own thread. Each task has a single multiple-producer, single-consumer (mpsc) channel. Both *indications* and *confirmations* from providers below and *requests* and *responses* from any users above are sent to this channel. The helper function grease::make_channel() makes sending and receiving objects of the correct type.

The goal is to develop tasks which each implement a well-defined layer of functionality, each with minimal coupling and extensive test cases.

# Example Application

To demonstrate the system, it includes the following example tasks:

```
+---------------------------+
|        Application        |
+-------------+-------------+
| HTTP        | Database    |
+-------------+-------------+
| Socket      |
+-------------+
```

* Application. All the application logic - the top level module. It is a *user*, but never a *provider*. It is also known as a *turning point* or a *data inter-working function* - a point where messages stop going up, turn around and start going down again. While we have a single turning point, a big system may have multiple turning points: perhaps one for control traffic and one for data traffic.
* HTTP. Decodes HTTP requests sent over a socket and formats HTTP responses.
* SocketServer. Handles TCP sockets in a *greasy* fashion.
* Database. Stores information which Application needs to service requests from remote systems.

Now, consider the limited number of changes required to implement the following example instead:

```
+---------------------------+
|        Application        |
+-------------+-------------+
| HTTP        | MySQL       |
+-------------+-------------+
| TLS v1.2    | Socket      |
+-------------+-------------+
| Socket      |
+-------------+
```

We've added a TLS layer between the HTTP task and the Socket task - the HTTP task might need to tell the TLS task which certificates to serve, but other than that, it should be able to treat a TLS and Socket identically. On the other side, the MySQL task would perhaps implement the same service interface as our earlier noddy 'Database' task but internally would contain the logic required to talk to a remote MySQL database via a socket. We're re-using the socket layer to then talk to that MySQL database.
