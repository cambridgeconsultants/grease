# Grease: A multi-threaded message-passing approach to writing stacks in Rust

## Introduction

Grease is designed to facilitate a message-passing based approach to stack development. Message passing between tasks, as opposed to function-call based stacks, has a number of advantages:

* Each task gets its own stack, which makes it easier to determine the maximum stack usage and hence the appropriate stack size.
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

Now, Grease is not necessaily suitable for implementing the entire stack from top to bottom. For example, your Operating System will usually provide a number of layers, especially for ubiquitous protocols like TCP/IP and Ethernet. The OS will probably also define those interfaces in terms of function calls rather than message passing. But if you are implementing a custom protocol stack for say, Bluetooth, or DECT, or LTE, or for a satellite modem, then Grease will help you do that, and as the 'socket' example task demonstrates, it is quite straightforward to develop a layer in Grease which interacts with a functional interface as the lower layer.

## The Four Messages

If we have some upper layer `N` and some lower layer `N-m` (e.g. `N-1`), we say that `Layer N-m` is the *provider* of a *service* and `Layer N` is the *user* of that *service*. In Grease, *Service users* and *service providers* exist in different threads and speak to each other using *messages*, passed via *channels*. That is, as opposed to a *service user* making a function call into some *service provider* module, as might occur in other sorts of stack implementation (such as the Berkeley socket API, or indeed most of the APIs your Operating System provides). There are four sorts of *message*:

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

Also note the assumption is that if a *service user* sends a *service provider* a *request*, the *service user* _will_ get a *confirmation*. To significantly simplify the code there should be no error handling for the case that one doesn't arrive. *Providers* must ensure that the *confirmation* is either fast (sent immediately in the routine that processes the *request*), or gated on a *confirmation* from a subsequent *service provider* below (which has the same constraints), or guaranteed in some other fashion (perhaps using a timeout). For example, if the *confirmation* is gated on an *indication* from a subsequent *service provider* then as that indication might never arrive a timeout must be used to ensure the confirmation is eventually sent.

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

Each layer in the system is implemented as a *task* and each *task* runs in its own thread. Each task has a single multiple-producer, single-consumer (mpsc) channel. Both *indications* and *confirmations* from providers below and *requests* and *responses* from any users above are sent to this channel.

The goal is to develop tasks which each implement a well-defined layer of functionality, each with minimal coupling and extensive test cases.


## Channel and Message objects

The *messages* are passed via *channels*. Currently grease makes use of the [`std::sync::mpsc::channel` module](https://doc.rust-lang.org/std/sync/mpsc/), but the creation of channels is wrapped up with the `make_channel` function. This returns [`std::sync::mpsc::Sender<grease::Message>`](https://doc.rust-lang.org/std/sync/mpsc/struct.Sender.html) with the type alias `MessageSender` and a [`std::sync::mpsc::Receiver<grease::Message>`](https://doc.rust-lang.org/std/sync/mpsc/struct.Receiver.html) with the type alias `MessageReceiver`. There must only ever be exactly one `MessageReceiver` for a given channel, and it is owned by a task which iterates on the `MessageReceiver` to receive messages and provide the appropriate service. Tasks use the `MessageSender` object to send messages to each other (either up or down the stack), and this object can be cloned and copied around as many tasks as require it. Unlike some systems, there is no central library of `MessageSender` objects, and they cannot not be found using some global static integer or other reference. If a task wishes to use a service, the *MessageSender* for that service must be passed in a task creation time as an argument, or passed down in a *request* from a higher layer. When a *service provider* receives a *request*, that request always includes a `MessageSender`, to be used for sending the corresponding *confirmation* back. It may also be appropriate to store a copy of the `MessageSender` for sending *indications* at a later date, depending on the design of the service.

The `grease::Message` type is an enumeration of `Request`, `Confirmation`, `Indication` and `Response`. Each of these enumerators has a single argument representing all of the messages of the corresponding type from all of the tasks in the system. Thus all messages in the system can be represented by the `Message` type. There are various levels of discrimination (message type, module name, message name) and at the message name level, the Message contains a boxed struct which contains the parmeters in the message (if any). To aid the construction of a `Message` from a message body, message bodies implement either the `RequestSendable` or `NonRequestSendable` traits, depending on whether the message is a Request (and hence has a `reply_to` field) or one of the other three types (which do not). This gives message bodies a `wrap()` function, which takes the message body and wraps it in a Message containing the appropriate discriminators. Because the trait implementations are largely boiler-plate, they are created using the macros `make_request!`, `make_indication!`, `make_confirmation!` and `make_response!`.

The best way to get a feel for how this fits together, is to look at one of the example tasks.

## Tasks

Tasks are functions which iterate over a `MessageReceiver` and then call an appropriate handler function based on the message received. For example:

```rust
use grease::{Confirmation, Indication, Message, MessageReceiver, MessageSender, Request};

struct TaskContext { ... };

fn main_loop(rx: MessageReceiver, tx: MessageSender) {
    let t = TaskContext::new(tx);
    for msg in rx.iter() {
        t.handle(msg);
    }
    panic!("This task should never die!");
}

impl TaskContext {
    fn handle(&mut self, msg: Message) {
        match msg {
            // This is the foo task, so we get Foo requests from above
            Message::Request(ref reply_to, Request::Foo(ref x)) => {
                self.handle_foo_req(x, reply_to)
            }
            // We use the bar service so we expect to get confirmations and indications from it
            Message::Confirmation(Confirmation::Bar(ref x)) => self.handle_bar_cfm(x),
            Message::Indication(Indication::Bar(ref x)) => self.handle_bar_ind(x),
            // If we get here, someone else has made a mistake
            _ => error!("Unexpected message in foo task: {:?}", msg),
        }
    }
}
```

## Logging

Once of the advantages of using a message passing system is the logging. Grease will log every message when it is dropped - that is, when the *service provider* has read it from the channel and finished processing it. This allows you to go back and analyse what happened in your program, step by step. For example:

```
http  : Received Message::Request(Request::Http(Request::Bind(BindReq { addr: "0.0.0.0", port: 80, urls: [ ... ] })));
socket: Received Message::Request(Request::Socket(Request::Bind(BindReq { addr: "0.0.0.0", port: 80 })));
http  : Received Message::Confirmation(Confirmation::Socket(Confirmation::Bind(BindCfm { result: SUCCESS, handle: 123 })));
app   : Received Message::Confirmation(Confirmation::Http(Confirmation::Bind(BindCfm { result: SERVER_STARTED_OK, handle: 456 })));
```

## Example Application

To demonstrate the system, it includes the following example tasks:

```
+---------------------------+
|        Application        |
+-------------+-------------+
| WebServ     | Database    |
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
* Database. Stores information which Application needs to service requests from remote systems.

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

We've added a TLS layer between the HTTP task and the Socket task - the HTTP task might need to tell the TLS task which certificates to serve, but other than that, it should be able to treat a TLS and Socket identically. On the other side, the MySQL task would perhaps implement the same service interface as our earlier noddy 'Database' task but internally would contain the logic required to talk to a remote MySQL database via a socket. We're re-using the socket layer to then talk to that MySQL database.
