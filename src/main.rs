extern crate futures;
extern crate hyper;
extern crate pretty_env_logger;
extern crate tokio_core;
extern crate tokio_io;

use futures::future::{FutureResult, BoxFuture, Either};
use futures::{Stream,Future,Sink,Poll};
use futures::sync::mpsc;
use futures::future::{loop_fn, Loop};

use tokio_core::reactor::{Core, Handle, Timeout};
use tokio_core::net::TcpListener;

use hyper::{Get, StatusCode};

use hyper::server::{Http, Service, Request, Response};

use hyper::mime;
use hyper::header::{ContentType, Connection, AccessControlAllowOrigin};
use hyper::Chunk;


use std::time::{Duration};
use std::convert::From;
use std::cell::RefCell;
use std::rc::Rc;


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


struct EventGen{
    tx: mpsc::Sender<mpsc::Sender<Result<Chunk,hyper::Error>>>,
}

impl Service for EventGen {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = BoxFuture<Response, Self::Error>;

    fn call(&self, req: Request) -> Self::Future {
        match (req.method(), req.path()) {
            (&Get, "/") => {
                let (tx, rx) = mpsc::channel(10);
                self.tx.clone().send(tx)
                    .map_err(|_| hyper::Error::Incomplete)//no other errors types allowed
                    .and_then(|_|{
                        Ok(Response::new()
                            .with_status(StatusCode::Ok)
                            .with_header(AccessControlAllowOrigin::Any)
                            .with_header(ContentType(mime::TEXT_EVENT_STREAM))
                            .with_header(Connection::keep_alive())   
                            .with_body(rx))
                    }) 
                    .boxed()    

            },            
            _ => {
                futures::future::ok(Response::new()
                    .with_status(StatusCode::NotFound))
                    .boxed()
            }
        }
    }
}

fn main() {
    pretty_env_logger::init().expect("unable to initialize the env logger");
    let addr = "127.0.0.1:7778".parse().expect("addres parsing failed");

    let mut lp = Core::new().expect("unable to initialize the main event loop");
    let handle = lp.handle();
    let sender_handle = lp.handle();

    let keeper_store:Rc<RefCell<Vec<mpsc::Sender<Result<Chunk, hyper::Error>>>>> = Rc::new(RefCell::new(Vec::new()));

    let sender_store = keeper_store.clone();
    let (tx, rx) = mpsc::channel(10);
/*
    let keeper = loop_fn((rx, keeper_store), |(rx,store)|{
        rx.into_future()
            .and_then(move |(item, rx)|{
                match item {
                    Some(item) => {
                         
                        store.borrow_mut().push(item); 
                        println!("abonent {} stored", store.borrow().len());
                        Ok(Loop::Continue((rx,store)))
                    },
                    None =>{
                        println!("keeper loop stopped"); 
                        Ok(Loop::Break((rx,store)))
                    }
                }        
            })
    }).map(|_| ()).map_err(|_| ());

    handle.spawn(keeper);


    let sender = loop_fn(sender_store, move |store|{
        let tstore = store.clone();
        let estore = store.clone();
        
        Timeout::new(Duration::from_secs(2), &sender_handle).unwrap().map_err(|_| ())
            .and_then(move |_|{
                let tx_iter = tstore.borrow().clone().into_iter()
                    .map(|tx| tx.send(Ok(Chunk::from(format!("event: userconnect\ndata: {{\"username\": \"Frodo\", \"time\": \"{:?}\"}}\n\n", std::time::SystemTime::now())))));
                futures::future::join_all(tx_iter).map_err(|e| {println!("{:?}", e); ()})
            })
            .and_then(move |_|{
                    Ok(Loop::Continue(store))
            })
            .or_else(move |_| Ok(Loop::Continue(estore)))
    }).map(|()| ());

    handle.spawn(sender);
*/
    fn geterr<T:std::fmt::Debug>(t:T){
        println!("{:?}", t);
    }

    let fu_to = Timeout::new(Duration::from_secs(2), &sender_handle).unwrap().map_err(geterr);//Item = ()
    let fu_rx = rx.into_future().map_err(geterr);//Item = (Option<_>, rx)

    let broker = loop_fn((fu_to, fu_rx, sender_store), move |(fu_to, fu_rx, store)|{
        let ihandle = sender_handle.clone(); 
        fu_to.select2(fu_rx)
            .map_err(|_| ())// !!!
            .and_then(move |done|{
                match done {
                    Either::A((_, fu_rx)) => Case::A({//sender
                        
                        let tx_iter = store.borrow().clone().into_iter()
                            .map(|tx| tx.send(Ok(Chunk::from(format!("event: userconnect\ndata: {{\"username\": \"Frodo\", \"time\": \"{:?}\"}}\n\n", std::time::SystemTime::now())))));
                        futures::future::join_all(tx_iter)
                            .map_err(geterr)
                            .and_then(move |_|        
                                futures::future::ok::<_,()>(Loop::Continue((
                                        Timeout::new(Duration::from_secs(2), &ihandle).unwrap().map_err(geterr),
                                        fu_rx, 
                                        store
                                )))
                            )
                             
                    }),
                        
                    
                    Either::B(((item, rx), fu_to)) => Case::B({//keeper
                        match item {
                            Some(item) => {
                                store.borrow_mut().push(item); 
                                println!("abonent {} stored", store.borrow().len());
                            },
                            None =>{
                                println!("keeper loop get None"); 
                            }
                        }       

                        futures::future::ok::<_,()>(Loop::Continue((
                            fu_to,
                            rx.into_future().map_err(geterr), 
                            store
                        )))
                    }),
                }              
        })
    });//.map(|()| ()).map_err(|_| ());

    handle.spawn(broker);

    let listener = TcpListener::bind(&addr, &lp.handle()).expect("unable to listen");
    println!("Listening on http://{} with 1 thread.", listener.local_addr().expect("local address show"));
    let srv = listener.incoming().for_each(move |(stream, addr)| {
        
        Http::new().bind_connection(&handle, stream, addr, EventGen{ tx:tx.clone() });
        Ok(())
    });

    lp.run(srv).expect("error running the event loop");
}
