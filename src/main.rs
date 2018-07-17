extern crate futures;
extern crate hyper;
extern crate tokio_core;
extern crate bytes;

use bytes::{BytesMut, Bytes, BufMut};

use futures::Stream;
use futures::future::{Future, Either, ok, loop_fn, Loop};
use futures::sync::mpsc;
use futures::sink::Sink;


use tokio_core::reactor::Timeout;

use hyper::{Get, StatusCode};
use hyper::server::{Http, Service, Request, Response};
use hyper::mime;
use hyper::header::{ContentType, Connection, AccessControlAllowOrigin};
use hyper::Chunk;

use std::time::Duration;
use std::io::Write;

// this fn replaces closures to avoid boxing in some cases
fn print_err<T:std::fmt::Debug>(t:T) {
    println!("{:?}", t);
}

struct EventService {
    tx_new: mpsc::Sender<mpsc::Sender<Result<Chunk,hyper::Error>>>,
}

impl Service for EventService {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = Box<Future<Item=Response, Error=Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        match (req.method(), req.path()) {
            (&Get, "/events") => { println!("request events");
                let (tx_msg, rx_msg) = mpsc::channel(10);
                Box::new(
                    self.tx_new.clone().send(tx_msg)
                        .and_then(|_|
                            Ok(Response::new()
                                .with_status(StatusCode::Ok)
                                .with_header(AccessControlAllowOrigin::Any)
                                .with_header(ContentType(mime::TEXT_EVENT_STREAM))
                                .with_header(Connection::keep_alive())   
                                .with_body(rx_msg))
                        )
                        .or_else(|_| Ok(Response::new().with_status(StatusCode::NotAcceptable)))
                )
            },

            (&Get, "/") => { 
                println!("request html");
                Box::new(ok(Response::new().with_status(StatusCode::Ok).with_body(HTML)))
            }

            (method, path) => { 
                println!("invalid request method: {:?}, path: {:?}", method, path);
                Box::new(ok(Response::new().with_status(StatusCode::NotFound)))
            }
        }
    }
}

fn main() {
    let addr = "127.0.0.1:7878".parse().expect("addres parsing failed");

    let (tx_new, rx_new) = mpsc::channel(10);

    let server = Http::new().bind(&addr, move || Ok(EventService{ tx_new: tx_new.clone() })).expect("unable to create server");
    let handle = server.handle();
    let handle2 = handle.clone();    

    let event_delay = Duration::from_secs(2);
    let start_time = std::time::Instant::now();

    let fu_to = Timeout::new(event_delay, &handle).unwrap().map_err(print_err);
    let fu_rx = rx_new.into_future().map_err(print_err);
    let clients:Vec<mpsc::Sender<Result<Chunk, hyper::Error>>> = Vec::new();
    
    let broker = loop_fn((fu_to, fu_rx, clients, 0), move |(fu_to, fu_rx, mut clients, event_counter)|{
        let handle = handle2.clone(); 
        fu_to.select2(fu_rx)
            .map_err(|_| ())
            .and_then(move |done|
                match done {
                    Either::A((_, fu_rx)) => Either::A({//send messages
                        let mut buf = BytesMut::with_capacity(512).writer();
                        write!(buf, "event: uptime\ndata: {{\"number\": \"{}\", \"time\": \"{}\"}}\n\n", event_counter, start_time.elapsed().as_secs()).expect("msg write failed");
                        let msg:Bytes = buf.into_inner().freeze();
                        let tx_iter = clients.into_iter()
                            .map(|tx| tx.send(Ok(Chunk::from(msg.clone()))));
                        futures::stream::futures_unordered(tx_iter)
                            .map(Some)
                            .or_else(|e| { println!("{:?} client removed", e); Ok::<_,()>(None)})
                            .filter_map(|x| x)
                            .collect()
                            .and_then(move |clients|
                                ok(Loop::Continue((
                                    Timeout::new(event_delay, &handle).unwrap().map_err(print_err),
                                    fu_rx, 
                                    clients,
                                    event_counter + 1
                                )))                            
                            )
                    }),
                        
                    Either::B(((item, rx_new), fu_to)) => Either::B({//register new client
                        match item {
                            Some(item) => {
                                clients.push(item); 
                                println!("client {} registered", clients.len());
                            },
                            None => println!("keeper loop get None"),
                        }       

                        ok(Loop::Continue((
                            fu_to,
                            rx_new.into_future().map_err(print_err), 
                            clients,
                            event_counter
                        )))
                    }),
                }              
            )
    });

    handle.spawn(broker);

    println!("Listening on http://{} with 1 thread.", server.local_addr().expect("unable to get local address"));
    server.run().expect("unable to run server");
}

static HTML:&str = &r#"<!DOCTYPE html>
<html>
  <head>
    <meta charset="UTF-8"> 
    <title>Rust Hyper Server Sent Events</title>
  </head>
  <body>
    <h1>Rust Hyper Server Sent Events</h1>
    <div id="sse-msg">
    </div>
    <script type="text/javascript">
      var evtSource = new EventSource("http://127.0.0.1:7878/events");

      evtSource.addEventListener("uptime", function(e) {
          var sseMsgDiv = document.getElementById('sse-msg');
          const obj = JSON.parse(e.data);
          sseMsgDiv.innerHTML += '<p>' + 'message number: ' + obj.number + ', time since start: ' + obj.time + '</p>';
          console.log(obj);
      }, false);
    </script>
  </body>
</html>
"#;