// Copyright (c) 2016 Anatoly Ikorsky
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use conn::futures::query_result::BinQueryResult;
use conn::futures::query_result::futures::Collect;
use conn::futures::query_result::ResultSet;
use conn::futures::query_result::UnconsumedQueryResult;
use conn::stmt::futures::Execute;
use conn::stmt::Stmt;
use errors::*;
use lib_futures::Async;
use lib_futures::Async::Ready;
use lib_futures::Future;
use lib_futures::Poll;
use value::FromRow;
use value::Params;


enum Step<R> {
    Execute(Execute),
    Collect(Collect<R, BinQueryResult>),
}

enum Out<R> {
    Execute(BinQueryResult),
    Collect((ResultSet<R, BinQueryResult>, Stmt)),
}

/// This future will execute statement, take first row of result and resolve to `Option<R>`.
///
/// It will call `from_row::<R>(row)` internally.
pub struct First<R> {
    step: Step<R>,
}

pub fn new<R: FromRow>(stmt: Stmt, params: Params) -> First<R> {
    First { step: Step::Execute(stmt.execute(params)) }
}

impl<R: FromRow> First<R> {
    fn either_poll(&mut self) -> Result<Async<Out<R>>> {
        match self.step {
            Step::Execute(ref mut fut) => Ok(Ready(Out::Execute(try_ready!(fut.poll())))),
            Step::Collect(ref mut fut) => Ok(Ready(Out::Collect(try_ready!(fut.poll())))),
        }
    }
}

impl<R: FromRow> Future for First<R> {
    type Item = (Option<R>, Stmt);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match try_ready!(self.either_poll()) {
            Out::Execute(query_result) => {
                self.step = Step::Collect(query_result.collect::<R>());
                self.poll()
            },
            Out::Collect((mut result_set, stmt)) => {
                let row = if result_set.0.len() > 0 {
                    Some(result_set.0.swap_remove(0))
                } else {
                    None
                };
                Ok(Ready((row, stmt)))
            },
        }
    }
}
