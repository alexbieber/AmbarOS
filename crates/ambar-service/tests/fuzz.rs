//! Property tests for the IPC surface: framing, request parsing and the
//! authorization function must be total (no panics) and agents must never
//! reach owner-only operations whatever their scopes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ambar_service::agents::{Agent, Principal, Scope, Target, authorize};
use ambar_service::protocol::{MAX_FRAME, Request, read_frame};
use proptest::prelude::*;

fn requests() -> impl Strategy<Value = Request> {
    let id = "[a-z0-9-]{1,12}";
    prop_oneof![
        Just(Request::Health),
        Just(Request::ListWorkflows),
        Just(Request::Shutdown),
        Just(Request::AgentList),
        Just(Request::ApprovalList),
        Just(Request::MemoryList),
        Just(Request::WhoAmI),
        id.prop_map(|id| Request::Enable { id }),
        id.prop_map(|id| Request::Disable { id }),
        id.prop_map(|id| Request::Pause { id }),
        id.prop_map(|id| Request::RunNow { id }),
        id.prop_map(|id| Request::Remove { id }),
        id.prop_map(|id| Request::Revoke { id }),
        id.prop_map(|id| Request::AgentRevoke { id }),
        id.prop_map(|id| Request::ApprovalDecide { id, allow: true }),
        id.prop_map(|id| Request::ProposalApprove {
            id,
            allow_sensitive: true,
            enable: true
        }),
        ".{0,40}".prop_map(|d| Request::Register {
            definition: d,
            grant: None,
            replace: true
        }),
        ".{0,40}".prop_map(|d| Request::Propose {
            definition: d,
            reason: String::new()
        }),
    ]
}

fn scopes() -> impl Strategy<Value = Vec<Scope>> {
    let one = prop_oneof![
        Just(Scope::Read),
        Just(Scope::Propose),
        Just(Scope::Memory),
        Just(Scope::Run(Target::All)),
        Just(Scope::Control(Target::All)),
        "[a-z]{1,6}".prop_map(|s| Scope::Run(Target::Id(s))),
        "[a-z]{1,6}".prop_map(|s| Scope::Control(Target::Id(s))),
    ];
    proptest::collection::vec(one, 0..7)
}

fn owner_only(r: &Request) -> bool {
    matches!(
        r,
        Request::Shutdown
            | Request::Enable { .. }
            | Request::Remove { .. }
            | Request::Revoke { .. }
            | Request::Register { .. }
            | Request::AgentList
            | Request::AgentRevoke { .. }
            | Request::AgentCreate { .. }
            | Request::ApprovalList
            | Request::ApprovalDecide { .. }
            | Request::ProposalApprove { .. }
            | Request::ProposalReject { .. }
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 3000, .. ProptestConfig::default() })]

    #[test]
    fn agents_never_reach_owner_operations(r in requests(), scopes in scopes()) {
        let agent = Principal::Agent(Agent {
            id: "agent-x".into(), name: "x".into(), description: String::new(), scopes,
            created_at: String::new(), last_seen: None, requests: 0, denied: 0, revoked: false,
        });
        if owner_only(&r) {
            prop_assert!(authorize(&agent, &r).is_err(), "{:?} allowed for an agent", r);
        }
    }

    #[test]
    fn arbitrary_json_requests_never_panic(s in ".{0,200}") {
        let _ = serde_json::from_str::<Request>(&s);
    }

    #[test]
    fn framing_handles_any_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(async {
            let mut r = tokio::io::BufReader::new(&bytes[..]);
            while let Ok(Some(line)) = read_frame(&mut r).await {
                assert!(line.len() <= MAX_FRAME && !line.contains('\n'));
            }
        });
    }
}

#[tokio::test]
async fn oversized_frames_are_rejected_without_buffering_them() {
    let big = vec![b'a'; MAX_FRAME * 3];
    let mut r = tokio::io::BufReader::new(&big[..]);
    assert!(read_frame(&mut r).await.is_err());
}
