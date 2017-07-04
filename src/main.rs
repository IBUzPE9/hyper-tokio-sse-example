extern crate futures;
extern crate hyper;
extern crate pretty_env_logger;
extern crate tokio_core;
extern crate tokio_io;
extern crate bytes;

use bytes::{BytesMut, Bytes, BufMut};

use futures::future::{BoxFuture, Either, ok as future_ok};
use futures::{Stream,Future,Sink,Poll};
use futures::sync::mpsc;
use futures::future::{loop_fn, Loop};

use tokio_core::reactor::{Core, Timeout};
use tokio_core::net::TcpListener;

use hyper::{Get, StatusCode};
use hyper::server::{Http, Service, Request, Response};
use hyper::mime;
use hyper::header::{ContentType, Connection, AccessControlAllowOrigin};
use hyper::Chunk;

use std::time::Duration;
use std::io::Write;


enum Case<A,B>{A(A), B(B)}

impl<A,B,I,E> Future for Case<A,B>
where 
    A:Future<Item=I, Error=E>,
    B:Future<Item=I, Error=E>,
{
    type Item = I;
    type Error = E;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error>{
        match *self {
            Case::A(ref mut a) => a.poll(),
            Case::B(ref mut b) => b.poll(),
        }
    }
}

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
    type Future = BoxFuture<Response, Self::Error>;

    fn call(&self, req: Request) -> Self::Future {
        match (req.method(), req.path()) {
            (&Get, "/events") => { println!("request events");
                let (tx_msg, rx_msg) = mpsc::channel(10);
                self.tx_new.clone().send(tx_msg)
                    .map_err(|_| hyper::Error::Incomplete)// other errors types disallowed by hyper
                    .and_then(|_|{
                        Ok(Response::new()
                            .with_status(StatusCode::Ok)
                            .with_header(AccessControlAllowOrigin::Any)
                            .with_header(ContentType(mime::TEXT_EVENT_STREAM))
                            .with_header(Connection::keep_alive())   
                            .with_body(rx_msg))
                    })
                    .boxed() 
            },

            (&Get, "/") => { println!("request html");
                future_ok(Response::new()
                    .with_status(StatusCode::Ok)
                    .with_body(HTML))
                    .boxed()
            }

            _ => { println!("invalid request");
                future_ok(Response::new()
                    .with_status(StatusCode::NotFound))
                    .boxed()
            }
        }
    }
}

fn main() {
    pretty_env_logger::init().expect("unable to initialize the env logger");
    let addr = "127.0.0.1:7878".parse().expect("addres parsing failed");

    let mut core = Core::new().expect("unable to initialize the main event loop");
    let handle = core.handle();

    let clients:Vec<mpsc::Sender<Result<Chunk, hyper::Error>>> = Vec::new();
    let (tx_new, rx_new) = mpsc::channel(10);

    let event_delay = Duration::from_secs(2);
    let start_time = std::time::Instant::now();

    let handle2 = core.handle();
    let fu_to = Timeout::new(event_delay, &handle2).unwrap().map_err(print_err);
    let fu_rx = rx_new.into_future().map_err(print_err);



    let broker = loop_fn((fu_to, fu_rx, clients, 0), move |(fu_to, fu_rx, mut clients, event_counter)|{
        let handle = handle2.clone(); 
        fu_to.select2(fu_rx)
            .map_err(|_| ())
            .and_then(move |done|
                match done {
                    Either::A((_, fu_rx)) => Case::A({//send messages
                        let mut buf = BytesMut::with_capacity(512).writer();
                        write!(buf, "event: uptime\ndata: {{\"number\": \"{}\", \"time\": \"{}\"}}\n\n", event_counter, start_time.elapsed().as_secs()).expect("msg write failed");
                        let msg:Bytes = buf.into_inner().freeze();
                        let tx_iter = clients.into_iter()
                            .map(|tx| tx.send(Ok(Chunk::from(msg.clone()))));
                        futures::stream::futures_unordered(tx_iter)
                            .map(|x| Some(x))
                            .or_else(|e| { println!("{:?} client removed", e); Ok::<_,()>(None)})
                            .filter_map(|x| x)
                            .collect()
                            .and_then(move |clients|
                                future_ok(Loop::Continue((
                                    Timeout::new(event_delay, &handle).unwrap().map_err(print_err),
                                    fu_rx, 
                                    clients,
                                    event_counter + 1
                                )))                            
                            )

                    }),
                        
                    Either::B(((item, rx_new), fu_to)) => Case::B({//register new client
                        match item {
                            Some(item) => {
                                clients.push(item); 
                                println!("client {} registered", clients.len());
                            },
                            None => println!("keeper loop get None"),
                        }       

                        future_ok(Loop::Continue((
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

    let listener = TcpListener::bind(&addr, &handle).expect("unable to listen");
    println!("Listening on http://{} with 1 thread.", listener.local_addr().expect("local address show"));

    let srv = listener.incoming().for_each(move |(stream, addr)| {
        Http::new().bind_connection(&handle, stream, addr, EventService{ tx_new: tx_new.clone() });
        Ok(())
    });

    core.run(srv).expect("error running the event loop");
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