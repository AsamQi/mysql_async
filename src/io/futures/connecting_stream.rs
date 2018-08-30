// Copyright (c) 2016 Anatoly Ikorsky
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use errors::*;
use io::{packet_codec::PacketCodec, Stream};
use lib_futures::failed;
use lib_futures::future::select_ok;
use lib_futures::future::SelectOk;
use lib_futures::Async;
use lib_futures::Async::Ready;
use lib_futures::Failed;
use lib_futures::Future;
use lib_futures::Poll;
use std::io;
use std::net::ToSocketAddrs;
use tokio::net::ConnectFuture;
use tokio::net::TcpStream;
use tokio_codec::Framed;

steps! {
    ConnectingStream {
        WaitForStream(SelectOk<ConnectFuture>),
        Fail(Failed<(), Error>),
    }
}

/// Future that resolves to a `Stream` connected to a MySql server.
pub struct ConnectingStream {
    step: Step,
}

pub fn new<S>(addr: S) -> ConnectingStream
where
    S: ToSocketAddrs,
{
    match addr.to_socket_addrs() {
        Ok(addresses) => {
            let mut streams = Vec::new();

            for address in addresses {
                streams.push(TcpStream::connect(&address));
            }

            if streams.len() > 0 {
                ConnectingStream {
                    step: Step::WaitForStream(select_ok(streams)),
                }
            } else {
                let err = io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "could not resolve to any address",
                );
                ConnectingStream {
                    step: Step::Fail(failed(err.into())),
                }
            }
        }
        Err(err) => ConnectingStream {
            step: Step::Fail(failed(err.into())),
        },
    }
}

impl Future for ConnectingStream {
    type Item = Stream;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match try_ready!(self.either_poll()) {
            Out::WaitForStream((stream, _)) => Ok(Ready(Stream {
                closed: false,
                codec: Framed::new(stream.into(), PacketCodec::new()),
            })),
            Out::Fail(_) => unreachable!(),
        }
    }
}
