use std::sync::Arc;
use std::net::ToSocketAddrs;

use futures;
use futures::Future;

use grpc::error::GrpcError;
use grpc::futures_grpc::{GrpcFutureSend, GrpcStreamSend};

use kvproto::pdpb::*;
use kvproto::pdpb_grpc::{PDAsync, PDAsyncServer};

use super::Case;

pub struct Server {
    _server: PDAsyncServer,
}

impl Server {
    pub fn run<A: ToSocketAddrs, C: Case + Send + Sync + 'static>(addr: A, case: Arc<C>) -> Server {
        let m = Mock { case: case };
        Server { _server: PDAsyncServer::new(addr, Default::default(), m) }
    }
}

#[derive(Debug)]
struct Mock<C: Case> {
    case: Arc<C>,
}

impl<C: Case> PDAsync for Mock<C> {
    fn GetMembers(&self, req: GetMembersRequest) -> GrpcFutureSend<GetMembersResponse> {
        let resp = match self.case.GetMembers(req) {
            Some(resp) => resp,
            None => unimplemented!(),
        };

        futures::future::ok(resp).boxed()
    }

    fn Tso(&self, _: GrpcStreamSend<TsoRequest>) -> GrpcStreamSend<TsoResponse> {
        unimplemented!()
    }

    fn Bootstrap(&self, _: BootstrapRequest) -> GrpcFutureSend<BootstrapResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }

    fn IsBootstrapped(&self, _: IsBootstrappedRequest) -> GrpcFutureSend<IsBootstrappedResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }

    fn AllocID(&self, _: AllocIDRequest) -> GrpcFutureSend<AllocIDResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
    }

    fn GetStore(&self, _: GetStoreRequest) -> GrpcFutureSend<GetStoreResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
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

    fn GetRegionByID(&self, _: GetRegionByIDRequest) -> GrpcFutureSend<GetRegionResponse> {
        futures::future::err(GrpcError::Other("unimpl")).boxed()
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
