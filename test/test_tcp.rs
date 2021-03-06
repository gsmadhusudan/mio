extern crate mio;
extern crate env_logger;

use std::io;
use std::io::prelude::*;
use std::net;
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

use {TryRead, TryWrite};
use mio::{Token, Ready, PollOpt};
use mio::deprecated::{EventLoop, Handler};
use mio::tcp::{TcpListener, TcpStream};

#[test]
fn accept() {
    struct H { hit: bool, listener: TcpListener }

    impl Handler for H {
        type Timeout = ();
        type Message = ();

        fn ready(&mut self, event_loop: &mut EventLoop<Self>, token: Token,
                 events: Ready) {
            self.hit = true;
            assert_eq!(token, Token(1));
            assert!(events.is_readable());
            assert!(self.listener.accept().is_ok());
            event_loop.shutdown();
        }
    }

    let l = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = l.local_addr().unwrap();

    let t = thread::spawn(move || {
        net::TcpStream::connect(&addr).unwrap();
    });

    let mut e = EventLoop::new().unwrap();

    e.register(&l, Token(1), Ready::readable(), PollOpt::edge()).unwrap();

    let mut h = H { hit: false, listener: l };
    e.run(&mut h).unwrap();
    assert!(h.hit);
    assert!(h.listener.accept().unwrap_err().kind() == io::ErrorKind::WouldBlock);
    t.join().unwrap();
}

#[test]
fn connect() {
    struct H { hit: u32 }

    impl Handler for H {
        type Timeout = ();
        type Message = ();

        fn ready(&mut self, event_loop: &mut EventLoop<Self>, token: Token,
                 events: Ready) {
            assert_eq!(token, Token(1));
            match self.hit {
                0 => assert!(events.is_writable()),
                1 => assert!(events.is_hup()),
                _ => panic!(),
            }
            self.hit += 1;
            event_loop.shutdown();
        }
    }

    let l = net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();

    let (tx, rx) = channel();
    let (tx2, rx2) = channel();
    let t = thread::spawn(move || {
        let s = l.accept().unwrap();
        rx.recv().unwrap();
        drop(s);
        tx2.send(()).unwrap();
    });

    let mut e = EventLoop::new().unwrap();
    let s = TcpStream::connect(&addr).unwrap();

    e.register(&s, Token(1), Ready::all(), PollOpt::edge()).unwrap();

    let mut h = H { hit: 0 };
    e.run(&mut h).unwrap();
    assert_eq!(h.hit, 1);
    tx.send(()).unwrap();
    rx2.recv().unwrap();
    e.run(&mut h).unwrap();
    assert_eq!(h.hit, 2);
    t.join().unwrap();
}

#[test]
fn read() {
    const N: usize = 16 * 1024 * 1024;
    struct H { amt: usize, socket: TcpStream }

    impl Handler for H {
        type Timeout = ();
        type Message = ();

        fn ready(&mut self, event_loop: &mut EventLoop<Self>, token: Token,
                 _events: Ready) {
            assert_eq!(token, Token(1));
            let mut b = [0; 1024];
            loop {
                if let Some(amt) = self.socket.try_read(&mut b).unwrap() {
                    self.amt += amt;
                } else {
                    break
                }
                if self.amt >= N {
                    event_loop.shutdown();
                    break
                }
            }
        }
    }

    let l = net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();

    let t = thread::spawn(move || {
        let mut s = l.accept().unwrap().0;
        let b = [0; 1024];
        let mut amt = 0;
        while amt < N {
            amt += s.write(&b).unwrap();
        }
    });

    let mut e = EventLoop::new().unwrap();
    let s = TcpStream::connect(&addr).unwrap();

    e.register(&s, Token(1), Ready::readable(), PollOpt::edge()).unwrap();

    let mut h = H { amt: 0, socket: s };
    e.run(&mut h).unwrap();
    t.join().unwrap();
}

#[test]
fn write() {
    const N: usize = 16 * 1024 * 1024;
    struct H { amt: usize, socket: TcpStream }

    impl Handler for H {
        type Timeout = ();
        type Message = ();

        fn ready(&mut self, event_loop: &mut EventLoop<Self>, token: Token,
                 _events: Ready) {
            assert_eq!(token, Token(1));
            let b = [0; 1024];
            loop {
                if let Some(amt) = self.socket.try_write(&b).unwrap() {
                    self.amt += amt;
                } else {
                    break
                }
                if self.amt >= N {
                    event_loop.shutdown();
                    break
                }
            }
        }
    }

    let l = net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();

    let t = thread::spawn(move || {
        let mut s = l.accept().unwrap().0;
        let mut b = [0; 1024];
        let mut amt = 0;
        while amt < N {
            amt += s.read(&mut b).unwrap();
        }
    });

    let mut e = EventLoop::new().unwrap();
    let s = TcpStream::connect(&addr).unwrap();

    e.register(&s, Token(1), Ready::writable(), PollOpt::edge()).unwrap();

    let mut h = H { amt: 0, socket: s };
    e.run(&mut h).unwrap();
    t.join().unwrap();
}

#[test]
fn connect_then_close() {
    struct H { listener: TcpListener }

    impl Handler for H {
        type Timeout = ();
        type Message = ();

        fn ready(&mut self, event_loop: &mut EventLoop<Self>, token: Token,
                 _events: Ready) {
            if token == Token(1) {
                let s = self.listener.accept().unwrap().0;
                event_loop.register(&s, Token(3), Ready::all(),
                                        PollOpt::edge()).unwrap();
                drop(s);
            } else if token == Token(2) {
                event_loop.shutdown();
            }
        }
    }

    let mut e = EventLoop::new().unwrap();
    let l = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
    let s = TcpStream::connect(&l.local_addr().unwrap()).unwrap();

    e.register(&l, Token(1), Ready::readable(), PollOpt::edge()).unwrap();
    e.register(&s, Token(2), Ready::readable(), PollOpt::edge()).unwrap();

    let mut h = H { listener: l };
    e.run(&mut h).unwrap();
}

#[test]
fn listen_then_close() {
    struct H;

    impl Handler for H {
        type Timeout = ();
        type Message = ();

        fn ready(&mut self, _: &mut EventLoop<Self>, token: Token, _: Ready) {
            if token == Token(1) {
                panic!("recieved ready() on a closed TcpListener")
            }
        }
    }

    let mut e = EventLoop::new().unwrap();
    let l = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();

    e.register(&l, Token(1), Ready::readable(), PollOpt::edge()).unwrap();
    drop(l);

    let mut h = H;
    e.run_once(&mut h, Some(Duration::from_millis(100))).unwrap();
}

fn assert_send<T: Send>() {
}

fn assert_sync<T: Sync>() {
}

#[test]
fn test_tcp_sockets_are_send() {
    assert_send::<TcpListener>();
    assert_send::<TcpStream>();
    assert_sync::<TcpListener>();
    assert_sync::<TcpStream>();
}

#[test]
fn bind_twice_bad() {
    let l1 = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = l1.local_addr().unwrap();
    assert!(TcpListener::bind(&addr).is_err());
}
