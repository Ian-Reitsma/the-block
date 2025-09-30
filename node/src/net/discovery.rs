#![forbid(unsafe_code)]

pub use p2p_overlay::Discovery as DiscoveryTrait;
pub use p2p_overlay::OverlayStore;
pub use p2p_overlay::PeerEndpoint as Multiaddr;
pub use p2p_overlay::PeerId as PeerIdTrait;

pub type PeerId = super::OverlayPeerId;

pub fn new(local: PeerId) -> Box<dyn DiscoveryTrait<Peer = PeerId, Address = Multiaddr> + Send> {
    super::overlay_service().discovery(local)
}
