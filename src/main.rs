extern crate futures;
extern crate hyper;
extern crate pretty_env_logger;
extern crate tokio_core;
extern crate tokio_io;

use futures::future::FutureResult;
use futures::{Stream,Future,Sink};
use futures::sync::mpsc;

use tokio_core::reactor::{Core, Handle};
use tokio_core::net::TcpListener;

use hyper::{Get, StatusCode};

use hyper::server::{Http, Service, Request, Response};

use hyper::mime;
use hyper::header::{ContentType, Connection, AccessControlAllowOrigin, TransferEncoding, ContentLength, Encoding};
use hyper::Chunk;

use hyper::header::{CacheDirective, CacheControl};

use std::time::{Duration};
use std::convert::From;


struct EventGen{
    stream: TcpStreamRc, 
    handle: Handle
}

impl Service for EventGen {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = FutureResult<Response, hyper::Error>;

    fn call(&self, req: Request) -> Self::Future {
        futures::future::ok(match (req.method(), req.path()) {
            (&Get, "/") => {

                let (tx, rx) = mpsc::channel(1000);

                //let events = futures::stream::once(Ok(Chunk::from("event: userconnect\ndata: {\"username\": \"Frodo\", \"status\": \"01:00:00\"}\n\n")));
                let tout05 = tokio_core::reactor::Timeout::new(Duration::from_secs(1), &self.handle)
                    .expect("unable to setup timeout")
                    .map_err(|_| ());
                let tout10 = tokio_core::reactor::Timeout::new(Duration::from_secs(2), &self.handle)
                    .expect("unable to setup timeout")
                    .map_err(|_| ());

                

                self.handle.spawn(
                    tout05.and_then(move|_| 
                            tx.send(Ok(Chunk::from("event: userconnect\ndata: {\"username\": \"Frodo\", \"time\": \"01:00:00\"}\n\n"))).map_err(|_| ())
                    )                        
                    .and_then(|tx| 
                            tout10.and_then(move |_| tx.send(Ok(Chunk::from("event: userconnect\ndata: {\"username\": \"Frodo\", \"time\": \"01:00:00\"}\n\n"))).map_err(|_| ()))
                    )
                    .map(|_| ())
                );


                Response::new()
                    .with_status(StatusCode::Ok)
                    .with_header(AccessControlAllowOrigin::Any)
                    .with_header(ContentType(mime::TEXT_EVENT_STREAM))
                    .with_header(Connection::keep_alive())   
                    .with_body(rx)                 

            },            
            _ => {
                Response::new()
                    .with_status(StatusCode::NotFound)
            }
        })
    }
}

fn main() {
    pretty_env_logger::init().expect("unable to initialize the env logger");
    let addr = "127.0.0.1:7778".parse().expect("addres parsing failed");

    let mut lp = Core::new().expect("unable to initialize the main event loop");
    let handle = lp.handle();
    let listener = TcpListener::bind(&addr, &lp.handle()).expect("unable to listen");
    println!("Listening on http://{} with 1 thread.", listener.local_addr().expect("local address show"));
    let srv = listener.incoming().for_each(move |(stream, addr)| {
        let stream = TcpStreamRc(Rc::new(stream));
        Http::new().bind_connection(&handle, stream.clone(), addr, EventGen{stream, handle:handle.clone()});
        Ok(())
    });

    lp.run(srv).expect("error running the event loop");
}



use std::rc::Rc;
use std::io::{Read, Write};
use std::io;
use tokio_core::net::TcpStream;
use tokio_io::{AsyncRead, AsyncWrite};
use futures::Poll;

#[derive(Clone)]
struct TcpStreamRc(Rc<TcpStream>);

impl Read for TcpStreamRc {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Write for TcpStreamRc {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&*self.0).flush()
    }
}

impl AsyncRead for TcpStreamRc {}

impl AsyncWrite for TcpStreamRc {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        println!("dont do it");
        self.0.shutdown(std::net::Shutdown::Write)?;
        Ok(().into())
    }
}