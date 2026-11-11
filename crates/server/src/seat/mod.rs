//! A seat is one viewer's in-process player: it owns a camera, a read-only
//! replica of the world and an armed placement tool. The viewer sends raw
//! input (pointer, wheel, keys); the seat turns it into the same wire
//! commands and presence packets any client would send, renders its own view
//! with the GPU renderer and streams the frames back as JPEG.
//!
//! The browser holds no game state. Everything it shows arrives as a frame
//! plus an optional text caption.
//!
//! Frame wire format (binary WebSocket message):
//! `u32 big-endian meta length | meta JSON | JPEG bytes`, with meta
//! `{"seq", "format":"jpeg", "w", "h"}` (w/h in frame pixels).
//! Text messages to the viewer: `{"type":"caption","text"}` and
//! `{"type":"error","message"}`.
//! Viewer input (text messages): see [`ViewerInput`]; x/y are frame pixels.

mod replica;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use city_protocol as wire;
use city_render::{Camera, Renderer, Scene};
use city_simulation::ActorId;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::hub::{HubMsg, OUTBOX_CAPACITY};
use crate::http::App;
use crate::runner::WorldRequest;
use replica::Replica;

pub(crate) const FRAME_WIDTH: u32 = 1280;
pub(crate) const FRAME_HEIGHT: u32 = 720;
/// At most this many frames per second per seat; frames are rendered only
/// when the picture changed.
const MAX_FPS: u64 = 30;
const DEFAULT_QUALITY: u8 = 75;
const HELP: &str = "1-9 select  R rotate  X demolish  Esc cancel  left click place  right-drag pan  wheel zoom";

/// Shared GPU renderer. Seats take turns: a frame is rendered and read back
/// under the lock, then encoded outside it.
pub(crate) type SharedRenderer = Arc<Mutex<Renderer>>;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ViewerInput {
    Pointer { action: PointerAction, x: f32, y: f32, button: u8 },
    Wheel { delta_y: f32 },
    Key { code: String },
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PointerAction {
    Move,
    Down,
    Up,
}

struct Tool {
    armed: Option<u64>,
    rotation: u8,
}

struct Seat {
    actor: ActorId,
    camera: Camera,
    replica: Replica,
    tool: Tool,
    /// Last pointer position in frame pixels.
    pointer: Option<[f32; 2]>,
    hover: Option<[i32; 2]>,
    /// (cell, armed definition, rotation) last announced as presence.
    last_presence: (Option<[i32; 2]>, Option<u64>, u8),
    panning: bool,
    next_command: u64,
    caption: String,
    caption_sent: String,
    dirty: bool,
}

/// What a viewer input asks the seat to send to the world owner.
enum Outbound {
    Command(wire::Command),
    Presence(wire::InputMessage),
}

impl Seat {
    fn new(actor: ActorId, replica: Replica) -> Self {
        let centre = [replica.grid[0] as f32 / 2.0, replica.grid[1] as f32 / 2.0];
        let mut seat = Seat {
            actor,
            camera: Camera::new(centre, 24.0),
            replica,
            tool: Tool { armed: None, rotation: 0 },
            pointer: None,
            hover: None,
            last_presence: (None, None, 0),
            panning: false,
            next_command: 0,
            caption: String::new(),
            caption_sent: String::new(),
            dirty: true,
        };
        seat.refresh_caption(None);
        seat
    }

    fn refresh_caption(&mut self, note: Option<&str>) {
        let armed = match self.tool.armed.and_then(|id| self.replica.definition_name(id)) {
            Some(name) => format!("Armed: {name} (rotation {})", self.tool.rotation * 90),
            None => "Nothing armed".to_owned(),
        };
        self.caption = match note {
            Some(note) => format!("{note} | {armed} | {HELP}"),
            None => format!("{armed} | {HELP}"),
        };
    }

    fn command_id(&mut self) -> String {
        self.next_command += 1;
        format!("{}-{}", self.actor.0, self.next_command)
    }

    fn ghost(&self) -> Option<(u64, [i32; 2], u8)> {
        Some((self.tool.armed?, self.hover?, self.tool.rotation))
    }

    /// Applies one viewer input and returns what to send to the world.
    fn handle_input(&mut self, input: ViewerInput) -> Vec<Outbound> {
        let mut out = Vec::new();
        match input {
            ViewerInput::Wheel { delta_y } => {
                self.camera = self.camera.zoomed((delta_y * 0.001).exp());
                self.update_hover(&mut out);
                self.dirty = true;
            }
            ViewerInput::Pointer { action, x, y, button } => {
                let previous = self.pointer.replace([x, y]);
                match action {
                    PointerAction::Down if button == 0 => {
                        self.update_hover(&mut out);
                        if let (Some(def), Some(cell)) = (self.tool.armed, self.hover) {
                            out.push(Outbound::Command(wire::Command::PlaceBuilding {
                                command_id: self.command_id(),
                                def_id: def.to_string(),
                                revision: crate::adapter::PUBLISHED_REVISION,
                                cell: wire::Cell { x: cell[0], y: cell[1] },
                                rotation: self.tool.rotation,
                            }));
                        }
                    }
                    PointerAction::Down => self.panning = true,
                    PointerAction::Up if button != 0 => self.panning = false,
                    PointerAction::Up => {}
                    PointerAction::Move => {
                        if self.panning {
                            if let Some([px, py]) = previous {
                                self.camera = self.camera.panned(x - px, y - py, FRAME_WIDTH, FRAME_HEIGHT);
                            }
                        }
                        self.update_hover(&mut out);
                    }
                }
                self.dirty = true;
            }
            ViewerInput::Key { code } => {
                self.handle_key(&code, &mut out);
                self.dirty = true;
            }
        }
        out
    }

    fn handle_key(&mut self, code: &str, out: &mut Vec<Outbound>) {
        match code {
            "Escape" => self.tool.armed = None,
            "KeyR" => self.tool.rotation = (self.tool.rotation + 1) % 4,
            "KeyX" | "Delete" => {
                if let Some(id) = self.hover.and_then(|cell| self.replica.building_at(cell)).cloned() {
                    out.push(Outbound::Command(wire::Command::DemolishBuilding {
                        command_id: self.command_id(),
                        building_id: id,
                    }));
                }
            }
            digit if digit.starts_with("Digit") => {
                let index = digit["Digit".len()..].parse::<usize>().ok().and_then(|n| n.checked_sub(1));
                let ids = self.replica.definition_ids();
                if let Some(id) = index.and_then(|i| ids.get(i)) {
                    self.tool.armed = Some(*id);
                }
            }
            _ => return,
        }
        self.refresh_caption(None);
        self.update_hover(out);
    }

    /// Recomputes the hovered cell and emits a presence packet when it, or
    /// the tool, changed.
    fn update_hover(&mut self, out: &mut Vec<Outbound>) {
        let cell = self
            .pointer
            .and_then(|[x, y]| self.camera.cell_at(FRAME_WIDTH, FRAME_HEIGHT, x, y))
            .filter(|cell| {
                cell[0] >= 0 && cell[1] >= 0 && (cell[0] as u32) < self.replica.grid[0] && (cell[1] as u32) < self.replica.grid[1]
            });
        self.hover = cell;
        let state = (cell, self.tool.armed, self.tool.rotation);
        if state == self.last_presence {
            return;
        }
        self.last_presence = state;
        if let Some(cell) = cell {
            let armed = self.tool.armed;
            out.push(Outbound::Presence(wire::InputMessage {
                kind: wire::InputKind::Move,
                cell: wire::Cell { x: cell[0], y: cell[1] },
                def_id: armed.map(|id| id.to_string()),
                rotation: armed.map(|_| self.tool.rotation),
            }));
        }
    }
}

/// Serves one viewer socket until it closes, the hub sheds it as a slow
/// consumer, or rendering fails. `seed` is the actor identity (same seed =
/// same player across reconnects).
pub(crate) async fn run(app: App, renderer: SharedRenderer, mut socket: WebSocket, seed: String) {
    let conn = app
        .next_conn
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (out_tx, mut out_rx) = mpsc::channel::<wire::ServerMessage>(OUTBOX_CAPACITY);
    if app
        .hub
        .send(HubMsg::Register { conn, out: out_tx })
        .await
        .is_err()
    {
        eprintln!("seat: connection {conn} could not register; runner gone");
        return;
    }
    let actor = ActorId(seed);
    let Some((world, presences)) = app.identify(conn, actor.clone()).await else {
        eprintln!("seat: connection {conn} runner unavailable during identify");
        let _ = app.hub.send(HubMsg::Unregister { conn }).await;
        return;
    };
    let mut seat = Seat::new(actor.clone(), Replica::new(actor.0.clone(), world, presences));

    if let Err(reason) = serve(&app, &renderer, &mut socket, &mut seat, conn, &mut out_rx).await {
        let text = serde_json::json!({ "type": "error", "message": reason }).to_string();
        let _ = socket.send(Message::Text(text.into())).await;
    }

    let _ = app.hub.send(HubMsg::Unregister { conn }).await;
    let _ = app
        .world_tx
        .send(WorldRequest::Disconnect { conn, actor })
        .await;
}

/// The seat loop. `Ok` = the viewer left; `Err` = a failure the viewer
/// should be told about.
async fn serve(
    app: &App,
    renderer: &SharedRenderer,
    socket: &mut WebSocket,
    seat: &mut Seat,
    conn: u64,
    out_rx: &mut mpsc::Receiver<wire::ServerMessage>,
) -> Result<(), String> {
    let quality = std::env::var("CITY_STREAM_QUALITY")
        .ok()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .filter(|q| (1..=100).contains(q))
        .unwrap_or(DEFAULT_QUALITY);
    let mut ticker = tokio::time::interval(Duration::from_millis(1000 / MAX_FPS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut seq: u64 = 0;
    loop {
        tokio::select! {
            message = out_rx.recv() => {
                // The hub dropped this seat: it could not keep up.
                let Some(message) = message else {
                    return Err("seat fell behind the world and was disconnected".into());
                };
                match message {
                    wire::ServerMessage::Event { event } => {
                        seat.dirty |= seat.replica.apply(event);
                    }
                    wire::ServerMessage::CommandResult { result: wire::CommandResult::Rejected { message, .. } } => {
                        seat.refresh_caption(Some(&format!("Rejected: {message}")));
                    }
                    wire::ServerMessage::CommandResult { .. }
                    | wire::ServerMessage::Hello { .. }
                    | wire::ServerMessage::HelloWorld { .. } => {}
                }
            }
            frame = socket.recv() => {
                let Some(Ok(frame)) = frame else { return Ok(()); };
                match frame {
                    Message::Text(text) => match serde_json::from_str::<ViewerInput>(&text) {
                        Ok(input) => {
                            for outbound in seat.handle_input(input) {
                                send_to_world(app, conn, seat, outbound).await?;
                            }
                        }
                        Err(err) => {
                            seat.refresh_caption(Some(&format!("Unreadable input: {err}")));
                        }
                    },
                    Message::Close(_) => return Ok(()),
                    _ => {}
                }
            }
            _ = ticker.tick() => {
                if seat.caption != seat.caption_sent {
                    let text = serde_json::json!({ "type": "caption", "text": seat.caption }).to_string();
                    socket.send(Message::Text(text.into())).await.map_err(|_| "viewer socket closed")?;
                    seat.caption_sent = seat.caption.clone();
                }
                if !seat.dirty {
                    continue;
                }
                seat.dirty = false;
                seq += 1;
                let boxes = seat.replica.boxes(seat.ghost());
                let grid = seat.replica.grid;
                let camera = seat.camera;
                let renderer = Arc::clone(renderer);
                let encoded = tokio::task::spawn_blocking(move || render_jpeg(&renderer, grid, camera, &boxes, quality, seq))
                    .await
                    .map_err(|err| format!("render task failed: {err}"))??;
                if socket.send(Message::Binary(encoded.into())).await.is_err() {
                    return Ok(());
                }
            }
        }
    }
}

async fn send_to_world(app: &App, conn: u64, seat: &Seat, outbound: Outbound) -> Result<(), String> {
    let request = match outbound {
        Outbound::Command(command) => {
            let canonical = crate::adapter::canonical_command(&command);
            WorldRequest::Command { conn, actor: seat.actor.clone(), command, canonical }
        }
        Outbound::Presence(input) => WorldRequest::Input { actor: seat.actor.clone(), input },
    };
    app.world_tx
        .send(request)
        .await
        .map_err(|_| "world runner unavailable".to_owned())
}

/// Renders one frame and packs it in the viewer wire format.
fn render_jpeg(
    renderer: &SharedRenderer,
    grid: [u32; 2],
    camera: Camera,
    boxes: &[city_render::Box3],
    quality: u8,
    seq: u64,
) -> Result<Vec<u8>, String> {
    let frame = {
        let mut renderer = renderer.lock().map_err(|_| "renderer lock poisoned".to_owned())?;
        renderer
            .render(&Scene { grid, camera, boxes })
            .map_err(|err| err.to_string())?
    };
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, quality)
        .encode(&frame.rgba, frame.width as u16, frame.height as u16, jpeg_encoder::ColorType::Rgba)
        .map_err(|err| format!("jpeg encode failed: {err}"))?;
    let meta = serde_json::json!({ "seq": seq, "format": "jpeg", "w": frame.width, "h": frame.height }).to_string();
    let mut message = Vec::with_capacity(4 + meta.len() + jpeg.len());
    message.extend_from_slice(&(meta.len() as u32).to_be_bytes());
    message.extend_from_slice(meta.as_bytes());
    message.extend_from_slice(&jpeg);
    Ok(message)
}
