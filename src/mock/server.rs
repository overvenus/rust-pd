// Copyright 2017 PingCAP, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::Arc;
use std::net::ToSocketAddrs;

use futures;
use futures::Future;

use grpc::error::GrpcError;
use grpc::futures_grpc::{GrpcFutureSend, GrpcStreamSend};

use kvproto::pdpb::*;
use kvproto::pdpb_grpc::{PDAsync, PDAsyncServer};

use super::Case;
use super::case::Service;
use super::case::Result;

pub struct Server {
    _server: PDAsyncServer,
}

impl Server {
    pub fn run<A, C>(addr: A, handler: Arc<Service>, case: Option<Arc<C>>) -> Server
        where A: ToSocketAddrs,
              C: Case + Send + Sync + 'static
    {
        let m = Mock {
            handler: handler,
            case: case,
        };
        Server { _server: PDAsyncServer::new(addr, Default::default(), m) }
    }
}

fn try_takeover<F, R, C: Case>(mock: &Mock<C>, f: F) -> GrpcFutureSend<R>
    where R: Send + 'static,
          F: Fn(&Case) -> Option<Result<R>>
{
    if let Some(ref case) = mock.case {
        match f(case.as_ref()) {
            Some(Ok(resp)) => return futures::future::ok(resp).boxed(),
            Some(Err(err)) => return futures::future::err(err).boxed(),
            _ => (),
        }
    }

    match f(mock.handler.as_ref()) {
        Some(Ok(resp)) => futures::future::ok(resp).boxed(),
        Some(Err(err)) => futures::future::err(err).boxed(),
        _ => futures::future::err(GrpcError::Other("unimpl")).boxed(),
    }
}

#[derive(Debug)]
struct Mock<C: Case> {
    handler: Arc<Service>,
    case: Option<Arc<C>>,
}

impl<C: Case> PDAsync for Mock<C> {
    fn GetMembers(&self, req: GetMembersRequest) -> GrpcFutureSend<GetMembersResponse> {
        try_takeover(self, |c| c.GetMembers(&req))
    }

    fn Tso(&self, _: GrpcStreamSend<TsoRequest>) -> GrpcStreamSend<TsoResponse> {
        unimplemented!()
    }

    fn Bootstrap(&self, req: BootstrapRequest) -> GrpcFutureSend<BootstrapResponse> {
        try_takeover(self, |c| c.Bootstrap(&req))
    }

    fn IsBootstrapped(&self, req: IsBootstrappedRequest) -> GrpcFutureSend<IsBootstrappedResponse> {
        try_takeover(self, |c| c.IsBootstrapped(&req))
    }

    fn AllocID(&self, req: AllocIDRequest) -> GrpcFutureSend<AllocIDResponse> {
        try_takeover(self, |c| c.AllocID(&req))
    }

    fn GetStore(&self, req: GetStoreRequest) -> GrpcFutureSend<GetStoreResponse> {
        try_takeover(self, |c| c.GetStore(&req))
    }

    fn PutStore(&self, _: PutStoreRequest) -> GrpcFutureSend<PutStoreResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }

    fn StoreHeartbeat(&self, _: StoreHeartbeatRequest) -> GrpcFutureSend<StoreHeartbeatResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }

    fn RegionHeartbeat(&self,
                       _: RegionHeartbeatRequest)
                       -> GrpcFutureSend<RegionHeartbeatResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }

    fn GetRegion(&self, _: GetRegionRequest) -> GrpcFutureSend<GetRegionResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }

    fn GetRegionByID(&self, req: GetRegionByIDRequest) -> GrpcFutureSend<GetRegionResponse> {
        try_takeover(self, |c| c.GetRegionByID(&req))
    }

    fn AskSplit(&self, _: AskSplitRequest) -> GrpcFutureSend<AskSplitResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }

    fn ReportSplit(&self, _: ReportSplitRequest) -> GrpcFutureSend<ReportSplitResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }

    fn GetClusterConfig(&self,
                        _: GetClusterConfigRequest)
                        -> GrpcFutureSend<GetClusterConfigResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }

    fn PutClusterConfig(&self,
                        _: PutClusterConfigRequest)
                        -> GrpcFutureSend<PutClusterConfigResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }
}
