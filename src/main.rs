use std::borrow::BorrowMut;
use std::io::{Read, Write};
use std::net::ToSocketAddrs;
use std::os::fd::AsRawFd;

const CLIENT: mio::Token = mio::Token(0);
const STDIN: mio::Token = mio::Token(1);
const SERVER: mio::Token = mio::Token(2);

fn main() {
  println!("[] PID {}", std::process::id());

  // create poll for new events
  let mut poll = mio::Poll::new().expect("[] ERROR: Couldnt create mio::Poll");
  // create storage for events
  let mut events = mio::Events::with_capacity(128);

  let mut server = None;
  let mut client = None;

  let mut args = std::env::args().skip(1);

  let arg = args.next().expect("[] ERROR: arg is required");
  let addr = args
    .next()
    .expect("[] ERROR: HOST:PORT is required")
    .to_socket_addrs()
    .expect("[] ERROR: Failed to parse socket address")
    .next()
    .expect("[] ERROR: Failed to parse socket address");

  let mut latest_token = 3;
  let mut client_streams= std::collections::HashMap::new();

  if arg == "--help" {
    println!(
      "{}{}{}{}",
      "Usage: cargo run -- [OPTION] [args]...",
      "\r\n\t--help\t\t\tprint this message",
      "\r\n\t--server HOST:PORT\tstart server",
      "\r\n\t--connect HOST:PORT\tconnect to server"
    );
    std::process::exit(0);
  } else if arg == "--server" {
    // setup server
    server = Some(mio::net::TcpListener::bind(addr).expect("[] ERROR: Failed to listen"));
    poll
      .registry()
      .register(server.as_mut().unwrap(), SERVER, mio::Interest::READABLE)
      .expect("[] ERROR: Failed to register server (network listener)");

    println!("Started server on port {}", addr.port());
  } else if arg == "--connect" {
    client = Some(mio::net::TcpStream::connect(addr).expect("[] ERROR: Failed to connect"));

    poll
      .registry()
      .register(client.as_mut().unwrap(), CLIENT, mio::Interest::READABLE | mio::Interest::WRITABLE)
      .expect("[] ERROR: Failed to register outbound connection");
  } else {
    println!("use --help to see available commands");
    std::process::exit(1);
  }

  let mut stdin = std::io::stdin();
  let mut stdout = std::io::stdout();

  let mut once = true;

  poll
    .registry()
    .register(&mut mio::unix::SourceFd(&stdin.as_raw_fd()), STDIN, mio::Interest::READABLE)
    .expect("[] ERROR: Failed to register stdin");

  while server.is_some() || client.is_some() {
    poll.poll(&mut events, Some(std::time::Duration::from_secs(1))).expect("[] ERROR: Failed to poll events");

    for event in events.iter() {
      match event.token() {
        SERVER => {
          let (mut stream, addr) = server
            .as_mut()
            .unwrap()
            .accept()
            .expect("[SERVER] ERROR: Failed to accept incoming connection");
          println!("[SERVER] LOG: user connected {:?}", addr);

          let token = mio::Token(latest_token);
          latest_token += 1;
          poll
            .registry()
            .register(stream.borrow_mut(), token, mio::Interest::READABLE)
            .expect("[SERVER] ERROR: Failed to register incoming connection");

          client_streams.insert(token, stream);
        }
        STDIN => {
          let mut message = vec![];

          let mut buf = [0_u8; 512];
          match stdin.read(&mut buf) {
            Ok(0) => {
              println!("break");
              // break;
            }
            Ok(len) => {
              message.append(&mut buf[0..len].to_vec());
            }
            Err(e) => {
              panic!("[CLIENT] ERROR: failed to read from stdin {:?}", e);
            }
          }

          match client {
            Some(ref mut connection) => {
              connection
                .write_all(&mut message)
                .expect("[CLIENT] ERROR: Failed to write to connection")
            }
            None => {
              println!("[CLIENT] WARN: No connection yet, input dropped")
            }
          }
        }
        CLIENT => {
          if event.is_writable() && once {
            once = false;
            println!("Connected to server on port {:?}", addr.port());
          }
        }
        token => {
          match client_streams.get_mut(&token) {
            Some(stream) => {
              let mut new_line = false;

              loop {
                let mut buf = [0_u8; 512];
                match stream.read(buf.as_mut()) {
                  Ok(0) => {
                    poll
                      .registry()
                      .deregister(stream)
                      .expect("[SERVER] ERROR: failed to deregister");
                    client_streams.remove(&token);
                    break;
                  }
                  Ok(len) => {
                    if !new_line {
                      stdout
                        .write_all(&format!("{:?}: ", token).into_bytes())
                        .expect("[CLIENT] ERROR: failed to write received string");
                    }
                    new_line = true;
                    stdout
                      .write_all(&buf[0..len])
                      .expect("[CLIENT] ERROR: failed to write received string");
                  },
                  Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // todo should this error occur?
                    break;
                  }
                  Err(e) => {
                    panic!("[CLIENT] ERROR: error, {:?}", e);
                  }
                }
              }
            }
            // We don't expect any events with tokens other than those we provided.
            None => panic!("Unexpected token")
          }
        }
      }
    }
  }
}
