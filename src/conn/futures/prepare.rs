// Copyright (c) 2016 Anatoly Ikorsky
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use Column;
use Conn;
use conn::futures::read_packet::ReadPacket;
use conn::futures::write_packet::WritePacket;
use conn::named_params::parse_named_params;
use conn::stmt::InnerStmt;
use conn::stmt::new_stmt;
use conn::stmt::Stmt;
use consts;
use errors::*;
use lib_futures::Async;
use lib_futures::Async::Ready;
use lib_futures::Failed;
use lib_futures::failed;
use lib_futures::Finished;
use lib_futures::finished;
use lib_futures::Future;
use lib_futures::Poll;
use proto::PacketType;
use std::mem;


steps! {
    Prepare {
        Failed(Failed<(), Error>),
        CachedStatement(Finished<(InnerStmt, Conn), Error>),
        WriteCommand(WritePacket),
        ReadCommandResponse(ReadPacket),
        ReadParamOrColumn(ReadPacket),
    }
}

/// Future that resolves to prepared `Stmt`.
pub struct Prepare {
    step: Step,
    params: Vec<Column>,
    columns: Vec<Column>,
    named_params: Option<Vec<String>>,
    query: String,
    stmt: Option<InnerStmt>,
}

pub fn new(conn: Conn, query: &str) -> Prepare {
    match parse_named_params(query) {
        Ok((named_params, query)) => {
            let query = query.into_owned();
            let step = if let Some(mut inner_stmt) =
                conn.stmt_cache.get(&query).map(Clone::clone) {
                inner_stmt.named_params = named_params.clone();
                Step::CachedStatement(finished((inner_stmt, conn)))
            } else {
                let future = conn.write_command_data(consts::Command::COM_STMT_PREPARE, &*query);
                Step::WriteCommand(future)
            };
            Prepare {
                step: step,
                named_params: named_params,
                query: query,
                params: Vec::new(),
                columns: Vec::new(),
                stmt: None,
            }
        },
        Err(err) => {
            Prepare {
                step: Step::Failed(failed(err)),
                named_params: None,
                query: String::new(),
                params: Vec::default(),
                columns: Vec::default(),
                stmt: None,
            }
        },
    }
}

impl Future for Prepare {
    type Item = Stmt;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match try_ready!(self.either_poll()) {
            Out::WriteCommand(conn) => {
                self.step = Step::ReadCommandResponse(conn.read_packet());
                self.poll()
            },
            Out::ReadCommandResponse((conn, packet)) => {
                let named_params = self.named_params.take();
                let inner_stmt = InnerStmt::new(packet.as_ref(), named_params)?;
                if inner_stmt.num_params > 0 || inner_stmt.num_columns > 0 {
                    self.params = Vec::with_capacity(inner_stmt.num_params as usize);
                    self.columns = Vec::with_capacity(inner_stmt.num_columns as usize);
                    self.stmt = Some(inner_stmt);
                    self.step = Step::ReadParamOrColumn(conn.read_packet());
                    self.poll()
                } else {
                    let stmt = new_stmt(inner_stmt, conn);
                    Ok(Ready(stmt))
                }
            },
            Out::ReadParamOrColumn((mut conn, packet)) => {
                if self.params.len() < self.params.capacity() {
                    let param = Column::new(packet, conn.last_command);
                    self.params.push(param);
                    self.step = Step::ReadParamOrColumn(conn.read_packet());
                    self.poll()
                } else if self.columns.len() < self.columns.capacity() {
                    if !packet.is(PacketType::Eof) {
                        let column = Column::new(packet, conn.last_command);
                        self.columns.push(column);
                    }
                    self.step = Step::ReadParamOrColumn(conn.read_packet());
                    self.poll()
                } else {
                    let mut inner_stmt: InnerStmt = self.stmt.take().unwrap();
                    inner_stmt.params = Some(mem::replace(&mut self.params, vec![]));
                    inner_stmt.columns = Some(mem::replace(&mut self.columns, vec![]));
                    conn.stmt_cache.insert(self.query.clone(), inner_stmt.clone());
                    let stmt = new_stmt(inner_stmt, conn);
                    Ok(Ready(stmt))
                }
            },
            Out::CachedStatement((inner_stmt, conn)) => {
                let stmt = new_stmt(inner_stmt, conn);
                Ok(Ready(stmt))
            },
            Out::Failed(_) => unreachable!(),
        }
    }
}
