#![allow(non_snake_case)]

use kvproto::pdpb::*;

mod leader_change;
pub use self::leader_change::LeaderChange;

pub trait Case {
    fn GetMembers(&self, _: GetMembersRequest) -> Option<GetMembersResponse> {
        None
    }

    fn Tso(&self, _: TsoRequest) -> Option<TsoResponse> {
        None
    }

    fn Bootstrap(&self, _: BootstrapRequest) -> Option<BootstrapResponse> {
        None
    }

    fn IsBootstrapped(&self, _: IsBootstrappedRequest) -> Option<IsBootstrappedResponse> {
        None
    }

    fn AllocID(&self, _: AllocIDRequest) -> Option<AllocIDResponse> {
        None
    }

    fn GetStore(&self, _: GetStoreRequest) -> Option<GetStoreResponse> {
        None
    }

    fn PutStore(&self, _: PutStoreRequest) -> Option<PutStoreResponse> {
        None
    }

    fn StoreHeartbeat(&self, _: StoreHeartbeatRequest) -> Option<StoreHeartbeatResponse> {
        None
    }

    fn RegionHeartbeat(&self, _: RegionHeartbeatRequest) -> Option<RegionHeartbeatResponse> {
        None
    }

    fn GetRegion(&self, _: GetRegionRequest) -> Option<GetRegionResponse> {
        None
    }

    fn GetRegionByID(&self, _: GetRegionByIDRequest) -> Option<GetRegionResponse> {
        None
    }

    fn AskSplit(&self, _: AskSplitRequest) -> Option<AskSplitResponse> {
        None
    }

    fn ReportSplit(&self, _: ReportSplitRequest) -> Option<ReportSplitResponse> {
        None
    }

    fn GetClusterConfig(&self, _: GetClusterConfigRequest) -> Option<GetClusterConfigResponse> {
        None
    }

    fn PutClusterConfig(&self, _: PutClusterConfigRequest) -> Option<PutClusterConfigResponse> {
        None
    }
}
