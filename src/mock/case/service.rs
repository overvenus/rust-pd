use std::sync::atomic::{AtomicUsize, Ordering};

use kvproto::pdpb::*;

use grpc::error::GrpcError;

use protobuf::RepeatedField;

use super::Case;
use super::Result;

const CLUSTER_ID: u64 = 42;

#[derive(Debug)]
pub struct Service {
    id_allocator: AtomicUsize,
    member_resp: GetMembersResponse,
}

impl Service {
    pub fn new(eps: Vec<String>) -> Service {
        Service {
            member_resp: Self::get_members_response(eps),
            id_allocator: AtomicUsize::new(1), // start from 1.
        }
    }

    fn header() -> ResponseHeader {
        let mut header = ResponseHeader::new();
        header.set_cluster_id(CLUSTER_ID);
        header
    }

    fn get_members_response(eps: Vec<String>) -> GetMembersResponse {
        let mut members = Vec::with_capacity(eps.len());
        for (i, ep) in (&eps).into_iter().enumerate() {
            let mut m = Member::new();
            m.set_name(format!("pd{}", i));
            m.set_member_id(100 + i as u64);
            m.set_client_urls(RepeatedField::from_vec(vec![ep.to_owned()]));
            m.set_peer_urls(RepeatedField::from_vec(vec![ep.to_owned()]));
            members.push(m);
        }

        let mut member_resp = GetMembersResponse::new();
        member_resp.set_members(RepeatedField::from_vec(members.clone()));
        member_resp.set_leader(members.pop().unwrap());
        member_resp.set_header(Self::header());

        info!("[Service] member_resp {:?}", member_resp);
        member_resp
    }
}

// TODO: Check cluster ID.
// TODO: Support more rpc.
impl Case for Service {
    fn GetMembers(&self, _: &GetMembersRequest) -> Option<Result<GetMembersResponse>> {
        Some(Ok(self.member_resp.clone()))
    }

    fn Bootstrap(&self, _: &BootstrapRequest) -> Option<Result<BootstrapResponse>> {
        Some(Err(GrpcError::Other("not unimpl")))
    }

    fn IsBootstrapped(&self, _: &IsBootstrappedRequest) -> Option<Result<IsBootstrappedResponse>> {
        Some(Err(GrpcError::Other("not unimpl")))
    }

    fn AllocID(&self, _: &AllocIDRequest) -> Option<Result<AllocIDResponse>> {
        let id = self.id_allocator.fetch_add(1, Ordering::SeqCst);
        let mut resp = AllocIDResponse::new();
        resp.set_id(id as u64);
        Some(Ok(resp))
    }

    fn GetStore(&self, _: &GetStoreRequest) -> Option<Result<GetStoreResponse>> {
        Some(Err(GrpcError::Other("not unimpl")))
    }

    fn GetRegionByID(&self, _: &GetRegionByIDRequest) -> Option<Result<GetRegionResponse>> {
        Some(Err(GrpcError::Other("not unimpl")))
    }
}
