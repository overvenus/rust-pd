use std::sync::Arc;
use std::sync::RwLock;
use std::sync::TryLockError;

use futures::Poll;
use futures::Async;
use futures::Future;
use futures::future::ok;
use futures::future::{self, loop_fn, Loop};

use kvproto::pdpb::GetMembersResponse;
use kvproto::pdpb_grpc::PDAsync;

use util::HandyRwLock;

use super::super::PdFuture;
use super::super::Result;
use super::super::Error;

#[derive(Debug)]
struct Bundle<C> {
    client: Arc<C>,
    members: GetMembersResponse,
}

pub struct LeaderClient<C> {
    // TODO: remove `GetMembersResponse`.
    members: GetMembersResponse,
    inner: Arc<RwLock<Bundle<C>>>,
}

impl<C: PDAsync> LeaderClient<C> {
    pub fn new(client: C, members: GetMembersResponse) -> LeaderClient<C> {
        LeaderClient {
            inner: Arc::new(RwLock::new(Bundle {
                client: Arc::new(client),
                members: members.clone(),
            })),
            members: members,
        }
    }

    // pub fn client(&self) -> GetClient<C> {
    //     GetClient { client: self.client.clone() }
    // }

    pub fn get_client(&self) -> Arc<C> {
        self.inner.rl().client.clone()
    }

    pub fn set_client(&self, client: C) {
        let mut bundle = self.inner.wl();
        bundle.client = Arc::new(client);
    }

    pub fn clone_client(&self) -> Arc<C> {
        self.inner.rl().client.clone()
    }

    pub fn get_members(&self) -> &GetMembersResponse {
        &self.members
    }

    pub fn set_members(&mut self, members: GetMembersResponse) {
        self.members = members;
    }
}

pub struct GetClient<C, R, F> {
    need_update: bool,
    retry_count: usize,
    client: Arc<RwLock<Arc<C>>>,
    resp: Option<Result<R>>,
    func: F,
}

impl<C, R, F> GetClient<C, R, F>
    where C: PDAsync + Send + Sync + 'static,
          R: Send + 'static,
          F: FnMut(Arc<C>) -> PdFuture<R> + Send + 'static
{
    pub fn new(retry: usize, client: Arc<RwLock<Arc<C>>>, f: F) -> GetClient<C, R, F> {
        GetClient {
            need_update: false,
            retry_count: retry,
            client: client,
            resp: None,
            func: f,
        }
    }

    fn get(self) -> PdFuture<GetClient<C, R, F>> {
        debug!("GetLeader get remains: {}", self.retry_count);

        let get_read = GetClientRead { inner: Some(self) };

        let ctx = get_read.map(|(mut this, client)| {
                let req = (this.func)(client);
                req.then(|resp| ok((this, resp)))
            })
            .flatten();

        ctx.map(|ctx| {
                let (mut this, resp) = ctx;
                match resp {
                    Ok(resp) => this.resp = Some(Ok(resp)),
                    Err(err) => {
                        this.retry_count -= 1;
                        error!("leader request failed: {:?}", err);
                    }
                };
                this
            })
            .boxed()
    }

    fn check(self) -> PdFuture<(GetClient<C, R, F>, bool)> {
        if self.retry_count == 0 || self.resp.is_some() {
            ok((self, true)).boxed()
        } else {
            // TODO: update client
            ok((self, false)).boxed()
        }
    }

    fn get_resp(self) -> Option<Result<R>> {
        self.resp
    }

    pub fn retry(self) -> PdFuture<R> {
        let this = self;
        loop_fn(this, |this| {
                this.get()
                    .and_then(|this| this.check())
                    .and_then(|(this, done)| {
                        if done {
                            Ok(Loop::Break(this))
                        } else {
                            Ok(Loop::Continue(this))
                        }
                    })
            })
            .then(|req| {
                match req.unwrap().get_resp() {
                    Some(Ok(resp)) => future::ok(resp),
                    Some(Err(err)) => future::err(err),
                    None => future::err(box_err!("fail to request")),
                }
            })
            .boxed()
    }
}

struct GetClientRead<C, R, F> {
    inner: Option<GetClient<C, R, F>>,
}

impl<C, R, F> Future for GetClientRead<C, R, F> {
    type Item = (GetClient<C, R, F>, Arc<C>);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let inner = self.inner.take().expect("GetClientRead cannot poll twice");
        let ret = match inner.client.try_read() {
            Ok(client) => Ok(Async::Ready(client.clone())),
            Err(TryLockError::WouldBlock) => Ok(Async::NotReady),
            // TODO: handle `PoisonError`.
            Err(err) => panic!("{:?}", err),
        };

        ret.map(|async| async.map(|client| (inner, client)))
    }
}

pub struct Request<C, R, F> {
    retry_count: usize,
    client: Arc<C>,
    resp: Option<Result<R>>,
    func: F,
}

impl<C, R, F> Request<C, R, F>
    where C: PDAsync + Send + Sync + 'static,
          R: Send + 'static,
          F: FnMut(&C) -> PdFuture<R> + Send + 'static
{
    pub fn new(retry: usize, client: Arc<C>, f: F) -> Request<C, R, F> {
        Request {
            retry_count: retry,
            client: client,
            resp: None,
            func: f,
        }
    }

    fn send(mut self) -> PdFuture<Request<C, R, F>> {
        debug!("request retry remains: {}", self.retry_count);
        let req = (self.func)(self.client.as_ref());
        req.then(|resp| {
                match resp {
                    Ok(resp) => self.resp = Some(Ok(resp)),
                    Err(err) => {
                        self.retry_count -= 1;
                        error!("request failed: {:?}", err);
                    }
                };
                ok(self)
            })
            .boxed()
    }

    fn receive(self) -> PdFuture<(Request<C, R, F>, bool)> {
        let done = self.retry_count == 0 || self.resp.is_some();
        ok((self, done)).boxed()
    }

    fn get_resp(self) -> Option<Result<R>> {
        self.resp
    }

    pub fn retry(self) -> PdFuture<R> {
        let retry_req = self;
        loop_fn(retry_req, |retry_req| {
                retry_req.send()
                    .and_then(|retry_req| retry_req.receive())
                    .and_then(|(retry_req, done)| {
                        if done {
                            Ok(Loop::Break(retry_req))
                        } else {
                            Ok(Loop::Continue(retry_req))
                        }
                    })
            })
            .then(|req| {
                match req.unwrap().get_resp() {
                    Some(Ok(resp)) => future::ok(resp),
                    Some(Err(err)) => future::err(err),
                    None => future::err(box_err!("fail to request")),
                }
            })
            .boxed()
    }
}
