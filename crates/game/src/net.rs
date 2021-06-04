use std::{
    cell::{Cell, RefCell},
    sync::Arc,
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender};
use game_common::{ClientPacket, ServerPacket};
use gloo_events::EventListener;
use gloo_timers::future::TimeoutFuture;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    RtcDataChannel, RtcIceCandidate, RtcIceCandidateInit, RtcSdpType, RtcSessionDescription,
};

use web_sys::{
    RtcConfiguration, RtcDataChannelInit, RtcDataChannelType, RtcPeerConnection,
    RtcSessionDescriptionInit,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

type Result<T> = std::result::Result<T, Error>;

macro_rules! js_object {
	($($key:expr, $value:expr),+) => {
		{

			let o = js_sys::Object::new();
			$(
				{
					let k = JsValue::from_str($key);
					let v = JsValue::from($value);
          unsafe {
            js_sys::Reflect::set(&o, &k, &v).unwrap();
          }
				}
			)*
			o
		}
	};
}

pub struct Client {
    http_client: reqwest::Client,
    peer: Arc<RtcPeerConnection>,
    channel: RtcDataChannel,
    on_error: EventListener,
    on_open: EventListener,
    on_message: EventListener,
    on_ice_candidate: EventListener,
    on_ice_connection_state_change: EventListener,
    message_tx: mpsc::UnboundedSender<ServerPacket>,
    message_rx: mpsc::UnboundedReceiver<ServerPacket>,
    ready_rx: Option<oneshot::Receiver<()>>,
}

impl Client {
    pub fn new() -> Self {
        let peer_configuration = {
            let mut config = RtcConfiguration::new();
            let urls = JsValue::from_serde(&["stun:stun.l.google.com:19302"]).unwrap();
            let server = js_object!("urls", urls);
            let ice_servers = js_sys::Array::new();
            ice_servers.push(&server);
            config.ice_servers(&ice_servers);
            config
        };
        let peer =
            Arc::new(RtcPeerConnection::new_with_configuration(&peer_configuration).unwrap());
        let on_ice_connection_state_change =
            EventListener::new(&peer, "iceconnectionstatechange", {
                let peer = peer.clone();
                move |e| {
                    debug!("ice state change: {:?}", peer.ice_connection_state());
                }
            });
        let (ready_tx, ready_rx) = oneshot::channel::<()>();
        let mut channel_init = RtcDataChannelInit::new();
        channel_init.ordered(false);
        channel_init.max_retransmits(0);
        let channel = peer.create_data_channel_with_data_channel_dict("data", &channel_init);
        channel.set_binary_type(RtcDataChannelType::Arraybuffer);
        let http_client = reqwest::Client::new();
        let on_error = EventListener::new(&channel, "error", move |e| {
            warn!("channel error {:?}", e);
        });
        let on_open = EventListener::once(&channel, "open", {
            move |e| {
                debug!("data channel opened");
                ready_tx.send(());
            }
        });
        let on_message = EventListener::new(&channel, "message", {
            move |e| {
                debug!("got message");
            }
        });
        let on_ice_candidate = EventListener::new(&peer, "icecandidate", move |e| {
            debug!("ice candidate event");
        });

        let (message_tx, message_rx) = mpsc::unbounded_channel();
        Self {
            ready_rx: Some(ready_rx),
            peer,
            channel,
            http_client,
            on_error,
            on_open,
            on_ice_candidate,
            on_message,
            on_ice_connection_state_change,
            message_rx,
            message_tx,
        }
    }

    pub async fn recv(&mut self) -> Option<ServerPacket> {
        self.message_rx.recv().await
    }

    pub fn send(&self, packet: &ClientPacket) {
        debug!("sending {:?}", packet);
        self.channel.send_with_u8_array(&packet.encode()).unwrap();
    }

    async fn wait_for(&mut self, matcher: impl Fn(&ServerPacket) -> bool) -> ServerPacket {
        loop {
            if let Some(packet) = self.recv().await {
                debug!("packet is {:?}", packet);
                if matcher(&packet) {
                    return packet;
                }
            }
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        debug!("creating peer offer");
        let offer = JsFuture::from(self.peer.create_offer()).await.unwrap();
        JsFuture::from(self.peer.set_local_description(&offer.unchecked_into()))
            .await
            .unwrap();
        let res = self
            .http_client
            .post("http://localhost:9000/new_session")
            .body(self.peer.local_description().unwrap().sdp())
            .send()
            .await?
            .json::<SessionResponse>()
            .await?;
        let description = {
            let mut init = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
            init.sdp(res.answer.get("sdp").unwrap().as_str().unwrap());
            init
        };
        let candidate = {
            let mut init =
                RtcIceCandidateInit::new(res.candidate.get("candidate").unwrap().as_str().unwrap());
            init.sdp_m_line_index(
                res.candidate
                    .get("sdpMLineIndex")
                    .unwrap()
                    .as_u64()
                    .map(|v| v as u16),
            );
            init.sdp_mid(res.candidate.get("sdpMid").unwrap().as_str());
            RtcIceCandidate::new(&init).unwrap()
        };
        JsFuture::from(self.peer.set_remote_description(&description))
            .await
            .unwrap();

        JsFuture::from(
            self.peer
                .add_ice_candidate_with_opt_rtc_ice_candidate(Some(&candidate)),
        )
        .await
        .unwrap();
        self.ready_rx.take().unwrap().await;

        self.send(&ClientPacket::Connect());

        let challenge = self
            .wait_for(|packet| matches!(packet, ServerPacket::ConnectChallenge { .. }))
            .await;

        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct SessionResponse {
    answer: serde_json::Value,
    candidate: serde_json::Value,
}
