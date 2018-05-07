# Grease: A multi-threaded message-passing approach to writing protocol stacks in Rust

## Introduction

Grease is designed to facilitate a message-passing based approach to protocol stack development. Message passing between tasks, as opposed to function-call based protocol stacks, has a number of advantages:

* Each task gets its own call-stack, which makes it easier to determine the maximum stack usage and hence the appropriate stack size.
* Message passing encourages an interface-first approach.
* Logging the messages as they are passed gives a low-cost but very useful mechanism for diagnosing bugs.
* There is no shared state between tasks, so Mutexes and other such constructs are not required. This simplifies the programming model considerably.
* Running multiple threads takes advantage of modern multi-core CPUs.

## Protocol Stacks

Protocol stacks are in built layers. In such a layered system, consider two layers, some upper layer `N` and some lower layer `N-m` (e.g. `N-1`). These layers can exchange messages (which we discuss in detail in the next section), up and down the stack. The purpose of a *service user* (some 'Layer N') exchanging a message with *service provider* (some lower 'Layer N-m'), is usually because 'Layer N' wishes to transfer some 'Layer N' level message (a *Protocol Data Unit*, or PDU) to another 'Layer N' in a remote system. It does this using some 'Layer N' specific protocol. Now the two layers mostly likely cannot communicate directly, because only the Physical Layer (Layer 0) can do that, so they use a lower layer to transport their messages. When the PDU is received by the lower layer, it is referred to as a *Service Data Unit* or SDU. This SDU may then be wrapped with some 'Layer N-m' specific headers and footers, and perhaps sliced into smaller pieces, to form a PDU.

Here's an example using HTTP over TCP/IP over Ethernet:

```
+-------------------+                                       +-------------------+
|                   |             Web Pages                 |                   |
| Web Browser       | <= = = = = = = = = = = = = = = = = => | Web Server        |
|                   |                                       |                   |
+-------------------+                                       +-------------------+
    |           ^                                               |           ^
    v           |                                               v           |
+-------------------+                                       +-------------------+
|                   |        HTTP Requests/Responses        |                   |
| HTTP              | <= = = = = = = = = = = = = = = = = => | HTTP              |
|                   |                                       |                   |
+-------------------+                                       +-------------------+
    |           ^                                               |           ^
    v           |                                               v           |
+-------------------+                                       +-------------------+
|                   |             TCP Stream                |                   |
| TCP               | <= = = = = = = = = = = = = = = = = => | TCP               |
|                   |                                       |                   |
+-------------------+                                       +-------------------+
    |           ^                                               |           ^
    v           |                                               v           |
+-------------------+                                       +-------------------+
|                   |             IP Packets                |                   |
| IP                | <= = = = = = = = = = = = = = = = = => | IP                |
|                   |                                       |                   |
+-------------------+                                       +-------------------+
    |           ^                                               |           ^
    v           |                                               v           |
+-------------------+                                       +-------------------+
|                   |           Ethernet frames             |                   |
| Ethernet MAC      | <= = = = = = = = = = = = = = = = = => | Ethernet MAC      |
|                   |                                       |                   |
+-------------------+                                       +-------------------+
    |           ^                                               |           ^
    v           |                                               v           |
+-------------------+                                       +-------------------+
|                   |          Twisted pair cable           |                   |
| Ethernet PHY      | <==================================>  | Ethernet PHY      |
|                   |                                       |                   |
+-------------------+                                       +-------------------+
```

If we take the top two layers, what the Web Browsers wants is a page from the Web Server. It does this, by sending a request in to the HTTP layer which effectively says 'GET this page from this server'. The HTTP layer wants to send a 'GET' request to the matching HTTP layer on the other side, so it uses the TCP layer to open a streaming socket and sends the GET request down the stream as ASCII text. The TCP layer segments the stream and passes the segments to the IP layer. The IP layer wraps each segments in an IP packet and passes the packets to the Ethernet MAC. The MAC wraps the packets with an Ethernet header containing a MAC address, and passes the resulting frames to the PHY. The PHY then encodes the Ethernet frame into a physical medium. On the receiving side, each layer will *indicate* to the layer above that some data has been received. In the case of TCP, several IP packets may need to be transferred back and forth before the payload from a TCP segment is ready to be passed up. At the top, the Web Server is told that a web page is required, so it obtains the appropriate page (quite like using a protocol stack), and then transmits the response back down the stack.

The benefit to designing systems with these stacks is that the Web Browser is agnostic to everything below HTTP. Well, it might make some assumptions that HTTP is always carried over TCP (such as using port numbers in URLs), but it is certainly agnostic to whether you use Ethernet, or ATM, or PPP over Serial, etc.

Of course, this is all textbook stuff - almost all communications systems are built in this fashion and there are a great many references texts on the subject. Grease is about using a multi-task message passing approach to building these systems.

Now, Grease is not necessaily suitable for implementing the entire stack from top to bottom. For example, your Operating System will usually provide a number of layers, especially for ubiquitous protocols like TCP/IP and Ethernet. The OS will probably also define those interfaces in terms of function calls rather than message passing. But if you are implementing a custom protocol stack for say, Bluetooth, or DECT, or LTE, or for a satellite modem, then Grease will help you do that, and as the 'socket' example task demonstrates, it is quite straightforward to develop a layer in Grease which interacts with a functional interface as the lower layer. Further, even if you are not developing a 'protocol stack' in a traditional sense, you may find your application benefits from a layered approach, and executing those layers as distinct tasks.

To demonstrate how this might work, imagine a basic web-application - perhaps a CMS. You might break down that system into the following tasks:

```
+---------------------------+
|        Application        |
+-------------+-------------+
| WebServ     | FileDatabase|
+-------------+-------------+
| HTTP        |
+-------------+
| Socket      |
+-------------+
```

* Application. All the application logic - the top level module. It is a *service user*, but never a *service provider*. It is also known as a *turning point* or a *data inter-working function* - a point where messages stop going up, turn around and start going down again. While we have a single turning point, a big system may have multiple turning points: perhaps one for control traffic and one for data traffic.
* WebServ. A basic REST server, implementing a small handful of URLs. Each GET or PUT to a URL results in an indication upwards.
* HTTP. Decodes HTTP requests sent over a socket and formats HTTP responses.
* SocketServer. Handles TCP sockets in a *greasy* fashion.
* FileDatabase. Stores the information which Application needs (i.e. our CMS's content), by writing it to the filesystem.

Now, consider the limited number of changes required to implement the following example instead:

```
+---------------------------+
|        Application        |
+-------------+-------------+
| WebServ     | MySQL       |
+-------------+-------------+
| HTTP        | Socket      |
+-------------+-------------+
| TLS v1.2    |
+-------------+
| Socket      |
+-------------+
```

We've added a TLS layer between the HTTP task and the Socket task - the HTTP task might need to tell the TLS task which certificates to serve, but other than that, it should be able to treat a TLS and Socket identically. On the other side, the MySQL task would perhaps implement the same service as our earlier noddy 'Database' task but internally would contain the logic required to talk to a remote MySQL database via a socket. We're re-using the socket layer to then talk to that MySQL database, and we could either share the same Socket task between two users or, if we really wanted, spin up two separate Socket tasks - each with a single user.

## The Four Messages

If we have some upper layer `N` and some lower layer `N-m` (e.g. `N-1`), we say that `Layer N-m` is the *provider* of a *service* and `Layer N` is the *user* of that *service*. In Grease, *Service users* and *service providers* exist in different threads and speak to each other using *messages* which are passed via boxed trait objects which usually wrap *channels*. That is, as opposed to a *service user* making a function call into some *service provider* module, as might occur in other sorts of stack implementation (such as the Berkeley socket API, or indeed most of the APIs your Operating System provides). There are four sorts of *message*:

1. Request (or REQ). A *service user* (the higher layer) sends a *request* to a *service provider* (the lower layer) to request that it 'do' something. It might be that the lower layer needs to perform an action, or to answer a question. Examples might include, open a new connection, or transmit some data on an existing connection.

2. Confirmation (or CFM). A *service provider* sends a *confirmation* back to a *service user* in reply to a *request*. Each *request* solicits exactly one *confirmation* in reply - it usually tells the *service user* whether the *request* was successful or not and includes any relevant results from the *request*.

3. Indication (or IND). Sometimes a *service provider* may wish to asynchronously indicate to the *service user* than an event has occured, without first being asked - this is an *indication*. For example, a *service provider* may wish to *indicate* that data has arrived on a connection, or perhaps that a connection is no longer in existence.

4. Response (or RSP). Sometimes (but not that often), it is necessary for a *service user* to respond to an *indication* in such a fashion that it would not make sense to use a *request*/*confirmation* pair. In this case, the *service user* may send a *response* to a service. An example might be a task which sends up received data indications - if the received data indications were sent up with no flow control, it might be possible for the system to run out of memory. If the lower layer requires that a *response* to be received for each *indication* before another *indication* can be sent, the rate of messages is limited by the receiver rather than the sender. It would be possible to use a *request/confirmation* pair in place of a *response*, but the corresponding *confirmation* would have no value.

Note that in all four cases, the messages are defined by the *service provider* and those message definitions are used by the *service user*. In Grease that means the messages are defined in the *service provider's* module, and imported using `use` statements in the *service user's* module.

```
+-------------------------+
|         Layer N         | (Service User)
+----|----------------|---+
     |    ^     ^     |
     |    |     |     |
  REQ| CFM|  IND|  RSP|
     |    |     |     |
     v    |     |     V
+---------|-----|---------+
|       Layer N-m         | (Service Provider)
+-------------------------+
```

Often it is useful to describe these interactions using a [Message Sequence Chart](https://en.wikipedia.org/wiki/Message_sequence_chart). Each task is a vertical line, where moving left to right is usually moving down the stack, and moving down the Y axis represents time moving forwards. Here we see a *request* into Layer N-1 triggers a *request* into Layer N-2, and so on.

```
Layer N         Layer N-1          Layer N-2
   |                 |                 |
   |      REQ        |                 |
   |================>|       REQ       |
   |                 |================>|
   |                 |       CFM       |
   |      CFM        |<= = = = = = = = |
   |<= = = = = = = = |                 |
   |                 |       IND       |
   |      IND        |<----------------|
   |<----------------|                 |
   |                 |                 |
   |      RSP        |                 |
   |- - - - - - - - >|       RSP       |
   |                 |- - - - - - - - >|
```

Also note the assumption is that if a *service user* sends a *service provider* a *request*, the *service user* _will_ get a *confirmation* at some point in time. To significantly simplify the code there should be no error handling for the case that one doesn't arrive. *Providers* must ensure that the *confirmation* is either fast (sent immediately in the routine that processes the *request*), or gated on a *confirmation* from a subsequent *service provider* below (which has the same constraints), or guaranteed in some other fashion (perhaps using a timeout). For example, if the *confirmation* is gated on an *indication* from a subsequent *service provider* then as that indication might never arrive a timeout must be used to ensure the confirmation is eventually sent.

## Fast vs Slow

In some cases, the *confirmation* for a *request* is fast:

* A user requests that a provider sends some data, for example.
* The provider sends a confirmation to the user, showing that the provider has received that data for sending (but has not necessarily yet transmitted it).
* Time passes.
* The provider sends an indication to the user to show that transmission is now complete.

In other cases, the *confirmation* for a *request* is slow:

* A user requests that a provider sends some data, for example.
* Time passes.
* The provider sends an confirmation to the user to show that transmission is now complete.

Deciding between these two models is something of an art form. The latter is simpler, but limits the ability of the *service user* to place multiple requests simultaneously - it is considered poor form to send multiple requests without gating each request on an earlier request's confirmation as if the provider is busy, you will just fill up its input queue. You may also have problems if the *service provider* needs to exchange multiple messages with a lower task before the next *request* can be serviced - you may find *confirmations* from below stuck in the channel behind *requests* from above that you aren't ready to process. You could remove these requests and store them in a separate queue, but as the logging of the messages is performed when they are dropped, you are deferring the logging and making debugging harder. Using fast requests is more complex to implement up front, but allows for flow control and is probably ultimately easier for the overall design.

# Implementation in Grease

Each layer in the system is implemented as a *task* and each *task* runs in its own thread. Each task typically has a single multiple-producer, single-consumer (mpsc) channel from `std::sync::mpsc` but other channels types are not precluded. Notably, when using `mio` it is necessary to use the `mio` channel type, which can make up the `mio` Event poll call when a message arrives. Both *indications* and *confirmations* from providers below and *requests* and *responses* from any users above are sent to this channel, via boxed trait objects. Once `impl Trait` is stable, we can remove the boxes, but for now, this is the best way to allow a *provider* to be used by a *user* without knowing at compile time what the user is.

The goal is to develop tasks which each implement a well-defined layer of functionality, each with minimal coupling and extensive test cases.


## Service, ServiceProvider and ServiceUser traits

Each layer implements `Request`, `Confirm`, `Indication` and `Response` enum types containing the messages for the service it provides. Rather than enforce a strict naming convention for these four messages, we instead only insist on a layer offering a single struct which then has the four messages types as Associated Types:

```rust,skt-grease
struct Service;
enum RequestMessage { FooReq }
enum ConfirmMessage { FooCfm }
enum Indications { BarInd }

impl grease::Service for Service {
  type Request = RequestMessage;
  type Confirm = ConfirmMessage;
  // obviously it's better to be consistent, but not mandatory
  type Indication = Indications;
  // if you don't need a response, don't have one!
  type Response = ();
}
```

Each layer must then also provide a type (usually called `Handle`) which implements the `ServiceProvider` trait typed on this custom `Service` type. The task initialisation function should return a `ServiceProviderHandle<Service>` which can then be passed to any task wishing to use that service.

With every `Service::Request`, the provider is also given a `ServiceUserHandle<Service>` which is boxed trait object implementing `ServiceUser`. The *user* of a service must implement `ServiceUser` with the *provider's* `Service` for each service it wishes to use. This indicates that the *user* is willing to accept the messages from the *providers*.

The `ServiceProviderHandle` and `ServiceUserHandle` objects may be cloned as necessary, for example so that one *provider* can be used by multiple *users*, or so that a *provider* may retain the `ServiceUserHandle` for later use (for example, for subsequent indications).

The best way to get a feel for how this fits together, is to look at one of the example tasks.

## Tasks

Internally, Tasks are functions which iterate over their internal channel and then call an appropriate handler function based on the message received. See the `grease-socket` and `grease-http` tasks for a fully working task implementation, or see their respective `examples` folder for some top-level App tasks which use `grease-http` and `grease-socket`.

## Logging

Once of the advantages of using a message passing system is the logging. If you `#[derive(Debug)]` you can log every message as it is received. This allows you to go back and analyse what happened in your program, step by step. For example:

```
http  : Received StartReq { addr: "0.0.0.0", port: 80, urls: [ ... ] };
socket: Received BindReq { addr: "0.0.0.0", port: 80 };
http  : Received BindCfm { result: SUCCESS, handle: 123 };
app   : Received StartCfm { result: SERVER_STARTED_OK, handle: 456 };
```

Note - we suggest messages are logged by the receiver, rather than the sender, which is why `http` above logs the `socket::BindCfm` message.

A more fully featured logging framework may be added in the future, rather than ad-hoc rendering of messages to `log` using `{:?}`.

## Final notes

Grease is currently a proof-of-concept. Basic TCP socket and HTTP functionality has been implemented as greasy tasks, and a couple of examples demonstrate usage of these tasks (or rather, the services they offer). We welcome constructive feedback and suggestions for improvements! You can also talk to us about how this design approach can help your project.

In particular, we await Rust 1.26 and the stablisation of `impl Trait`. The author is hopeful that this will reduce the need to `Box` the ServiceUser and ServiceProvider traits quite so much.

-- Jonathan Pallant <jonathan.pallant@cambridgeconsultants.com>

This repository is Copyright (C) Cambridge Consultants 2018, and available under either the MIT and Apache-2.0 licences. Please see the <COPYRIGHT> file. This README.md is licensed under [Creative Commons BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/).
