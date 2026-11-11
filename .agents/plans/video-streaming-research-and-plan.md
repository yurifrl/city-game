# Streamed multiplayer rendering: research and plan

Date: 2026-11-10. Status: proposal, nothing implemented. Supersedes the first version of this file.

## 1. Constraints and goal

- **Fixed:** the game is delivered to players as video. No native client; browser viewers. Input returns to the server.
- **Goal:** many players, one authoritative simulation, minimum server resource per player, and as much CPU as possible left for the simulation.
- **Repo rules that bind the design** (`AGENTS.md`): Rust owns game rules; clients submit intent only; the simulation does not depend on graphics; designers and the game share one rendering pipeline and we do not maintain two product renderers; solo and multiplayer use one command path; no silent fallbacks.

## 2. Today (verified in repo)

`apps/stream` runs warm headless Chromium seats (SwiftShader, CPU only), captures with CDP `Page.startScreencast` as JPEG (quality 60), relays over WebSocket to a `<canvas>`, and injects input through CDP. Viewers of one seat share one capture. The chart gives the stream container 2 CPU and runs 2 seats. Public path is Cloudflare Access -> Istio -> bridge.

## 3. Core insight: what scales with players

One simulation with players as viewports does cut cost, but only part of it.

| Cost | Shared across views? | Scales with N players |
|---|---|---|
| Simulation | yes, runs once | constant |
| Scene/render-state conversion, crowd animation state, instance buffers, static assets in VRAM | yes, if all views live in one process and one GPU context | roughly constant |
| Visibility culling, draw submission | per view (can be batched in one compute pass) | linear, small |
| Rasterization, lighting, post-processing | no, per pixel | linear in total pixels |
| Video encode | no, one stream per distinct camera | linear in total pixels (cheap for static scenes with idle skipping) |
| Spectators on the same camera | yes, one render and one encode | constant |

Evidence (all with caveats): Capsule (arXiv 2506.11483) shares one server context across players and reports, at 4 players, 1.43x less GPU, 3.11x less VRAM, 3.7x less CPU, 3.87x less RAM. Its workloads are separate game sessions, not one shared simulation, so treat it as a loose upper bound. The same paper states GPU computation is shared less than other resources because each player has a different view. Expect VRAM, CPU and simulation cost near constant and GPU render plus encode near linear. The saving from sharing is mostly in what a Chromium-per-seat design repeats: its own browser, GPU and renderer processes, assets and state copy per instance.

Not quantified anywhere I found: the actual GPU saving for independent free cameras, shadow-map sharing, multi-camera culling cost, impostor savings for a zoomed-out camera.

## 4. Target architecture

```
sim host (pinned CPU cores, no rendering)
   | authoritative state, command ordering, tick phases
   v
replication: chunk revisions, camera-scoped subscriptions, deltas
   v
render node (GPU)  -- a read-only subscriber with the same protocol as any client
   |   renders N views from one scene, per-view culling/LOD
   |   encodes one stream per distinct camera
   v
transport: signalling on existing authenticated WebSocket; media direct UDP; WebSocket+WebCodecs fallback on 443
   v
viewer (browser): decode, draw, local cursor/ghost; input -> same command path
```

Rules that follow from `AGENTS.md`:
- The render node holds no authority: it reads projections and cannot assign simulation results. It must not read live simulation memory (single-writer ownership).
- Input from a viewer reaches the simulation through the same command path used by solo play and the direct client.
- Streaming rate is decoupled from simulation tick: updates are tick-stamped and the renderer interpolates.
- Dedicated cores for the simulation host. Render nodes are separate processes, preferably separate machines, so rendering load cannot starve the tick.

## 5. Research findings by area

Confidence: **V** primary source or repo, **S** secondhand/single secondary source, **U** unverified or the research agent's inference. The research agents ran few searches each and often read only snippets, so numbers need checking against sources before being relied on.

### 5.1 How others stream
- Sunshine/Moonlight (V): RTSP + RTP/UDP, Reed-Solomon FEC (20% default), reference-frame invalidation, H.264/HEVC/AV1, many capture and encoder backends. No browser client.
- Parsec (V, vendor blog): UDP protocol (BUD) natively; in the browser it uses WebRTC data channels, fragmented MP4 into MSE in low-delay mode, Chrome only, a locally drawn cursor, input on a separate binary channel. Closest precedent to our no-native-client constraint.
- Stadia (S): standard WebRTC, H.264/VP9, Chrome. GeForce NOW, xCloud, Luna, Shadow, Steam Remote Play (U): no primary-source details; do not quote latency numbers for any of them.

### 5.2 Papers
- Latency tolerance: god-view games are the most tolerant class (Claypool 2005, CACM 2006). No study covers city-builders; our latency budget is an engineering target.
- Pointing: delay hurts moving targets from ~50 ms (Claypool 2020). Hover and placement-ghost feedback is the most sensitive element; draw cursor and ghost locally.
- Frame rate: below ~15 fps performance degrades in action games (Claypool 2006/2009).
- Pipeline reference: Nebula (arXiv 2201.07738): 130 ms motion-to-photon design budget, 20.8 ms encode, 9.1 ms decode (software VP8 1080p30). GCC under-used a jittery link there; Salsify shows per-frame rate control helps.
- Codec: H.264 low-latency (no B-frames, intra-refresh) is the safe baseline (CAVE, MMSys 2019). HEVC ~2.6x slower to encode (QoMEX 2017). Software AV1 not real-time friendly (S).
- No found study: MJPEG vs H.264 on low-motion content, slice vs intra-refresh, low-latency AV1 on screen content.

### 5.3 Render node options
- **A. Chromium on a GPU node.** Works with explicit flags (`--headless=new`, ANGLE Vulkan or GL/EGL, driver-matching Vulkan ICD, NVIDIA `graphics` capability) (S). Verify with `chrome://gpu`; make the renderer fail loudly on a CPU adapter. Chromium has no NVENC path on Linux, hardware encode is VA-API only and has reported limits (30 fps cap, a crash report) (S, U). So on NVIDIA, capture frames from the browser and encode outside it (the Selkies pattern: `nvh264enc`, zero-copy capture where possible). No published instances-per-GPU or VRAM-per-instance numbers; one report shows screencast at 2.8-8.8 CPU cores per instance (S, unknown hardware). One anecdote says `canvas.captureStream` breaks under GPU rendering (S), which weakens the in-page capture idea from the first version of this plan.
- **B. Native Rust wgpu renderer on the node.** Feasible; render side is the easy part (V/S). Gaps: wgpu has no texture-export API, so zero-copy encode needs unsafe wgpu-hal + ash code pinned to a wgpu version; the closest ready crate is `vk-video`/`gpu-video` (H.264/H.265 from NV12 wgpu textures, very early, no AV1); an NVENC export recipe exists (moq PR #6805). Vulkan Video encode support varies by vendor and driver; AV1 is least certain. Silent fallback to the CPU Vulkan driver (lavapipe) is a known container failure. One renderer compiled native and to WASM/WebGPU is credible for the designer tools, within the WebGPU feature subset; no shipped complex game doing both was found. **Cost: it implies replacing Babylon.js in the game and the designers together, which nobody has sized.**
- Shared work for either option: one shared instance buffer with per-view visible-index and indirect-draw buffers, one compute pass culling all views, vertex-animation textures for crowds (animation still runs per view per vertex), aggressive per-view LOD/impostors for a zoomed-out camera. VR multiview does not fit unrelated cameras. Cities: Skylines 2 critiques (third-party only) say it is GPU-bound from missing culling and LODs; with N views that multiplies by N.

### 5.4 Replication (the render node depends on it)
- Workload looks like a spatial database more than an action game (S, agent synthesis): chunked world (e.g. 32x32 tiles), per-chunk revision, camera-scoped subscription, snapshot on subscribe, tick-stamped deltas, fresh snapshot if a client falls behind the retained window.
- Gaffer on Games (V): quantization and delta against a baseline cut a 901-cube stream from ~17.4 Mbps to ~4.3 Mbps and a stationary scene to ~15 kbps; priority accumulator under a per-client byte budget. Savings come from not sending unchanged state.
- Lockstep (Factorio) does not fit: every client runs the full simulation and determinism is required (`AGENTS.md` refuses to claim it). Lesson that carries: commands in, state out.
- Citizens: send paths and let the renderer integrate; far crowds as per-chunk density/flow with identity kept in the simulation (U, no numbers).
- Co-located render node: same protocol over loopback or a Unix socket first; shared memory only after profiling (U).
- Wire format: one schema source for Rust and TypeScript; runtime-validated schema-generated messages for untrusted commands; a dense per-chunk batch format for bulk state. FlatBuffers benchmark figures were from old C++ runs; browser decode speed is unquantified.
- Bevy replicon and lightyear are coupled to Bevy ECS, which conflicts with the stable-domain-ID rule; borrow ideas, not crates.
- No real game gave bandwidth per citizen or building.

### 5.5 Transport and ingress
- Cloudflare's proxy carries HTTP(S) and WebSocket, not WebRTC media UDP; Tunnel public hostnames do not carry UDP; Spectrum likely needs a paid plan (S, community sources). Istio does not proxy arbitrary UDP and does not implement UDPRoute (S). Caution: Gateway API v1.6 CRDs with Istio's alpha gateway flag reportedly kept istiod from becoming ready (Istio PR #62493, unresolved status unknown); do not upgrade those CRDs blindly.
- Recommended shape (S, agent ranking): signalling on the existing authenticated WebSocket; media over direct UDP on one fixed port using ICE-lite and NAT 1:1 host candidates, exposed with hostPort/NodePort outside Istio and port-forwarded at the router on a DNS-only hostname; own TURN (or Cloudflare Realtime TURN at ~$0.05/GB beyond the free tier) for UDP-blocked networks; session token issued over signalling, TURN with TTL credentials; rate-limit the public UDP port.
- Always ship a WebSocket + WebCodecs fallback over 443: no UDP or TURN needed, works behind Cloudflare, but TCP head-of-line blocking after loss means lower bitrate or short GOPs. WebTransport is not available behind Cloudflare per old community threads; re-check.
- The media server must stay on the node holding session state; a UDP NodePort across replicas does not preserve that.

### 5.6 Cost and capacity (prices from third-party trackers; verify live)
- SwiftShader: reports of 4-6 cores per instance, 10-60x slower than GPU, ~5 cores idle (S). Planning figure 5-7 vCPU per seat (U). llvmpipe ~49% less CPU in one benchmark (S).
- NVENC: consumer GeForce cards 8 concurrent sessions (V, NVIDIA app note); datacenter cards no cap (S); unconfirmed for Hetzner's RTX 4000 SFF Ada.
- Prices (S): Hetzner GEX44 ~EUR 184/mo plus setup fee; AWS g6.xlarge $0.805/h (~$315/mo spot-equivalent figure from the same tracker); used Arc A380 ~$120-200 (encode only, weak render).
- Seats per GPU: no public numbers; "8 per RTX 4000-class card" is an assumption (U).

## 6. Decision: render-node implementation

Do not choose between A and B yet. Run the same measurements on A first (cheap, one renderer kept), then decide on B with data.

Choose B only if all hold after the spikes: (1) A's seats per GPU or CPU per seat misses the capacity target, (2) the per-frame (shareable) share of render time is large enough for multi-view sharing to matter, (3) the Babylon-to-wgpu migration cost has been sized and accepted, (4) a wgpu export-and-encode spike meets the encode and latency budget. If A passes (1), stay on A and revisit only for capacity.

## 7. Phases (each phase ends with written measurements before the next starts)

**Phase 0: baseline and GPU test bench.** Instrument the current seat: render ms per frame, capture/encode ms, bytes per frame, CPU per process, click-to-photon split into capture, encode, network, decode, display (p50/p95/p99), at idle, camera pan, dense city, placement drag. Rent a GPU box by the hour (not buy) and repeat with Chromium on GPU at 1, 4, 8, 16 instances: `nvidia-smi` VRAM and utilization, CPU cores, RAM, fps, hardware, driver. Acceptance: a table that answers CPU per seat and seats per GPU.

**Phase 1: cheap fixes on the current path.** Ack screencast frames only when the viewer socket drains and drop per viewer; gate render and capture when idle (keyframe on resume); test llvmpipe vs SwiftShader. Acceptance: idle CPU lower than Phase 0; no memory growth under a slow viewer; e2e passes. These keep the CPU-only path usable as a fallback.

**Phase 2: replication protocol.** Chunk revisions, camera-scoped subscriptions, deltas with a per-client byte budget, snapshot on subscribe or lag, schema-generated wire types. The render node connects as an ordinary read-only subscriber over loopback. Benchmark: 50k entities, 5% change per tick, 1/10/100 clients, measuring server encode CPU, bytes per client, browser decode time. Update `docs/architecture.md` and the stream contract in the same change.

**Phase 3: GPU render-node pilot (option A).** Chromium on GPU with a boot-time adapter check that refuses CPU adapters unless explicitly requested; external capture and hardware encode (NVENC on NVIDIA, VA-API on Intel/AMD); one stream per distinct camera, spectators share a stream. Acceptance: N concurrent distinct views on one GPU with measured density, latency p95, simulation tick time unaffected.

**Phase 4: transport.** Signalling token over the existing WebSocket; direct UDP single-port media with ICE-lite; TURN for blocked networks; WebSocket + WebCodecs fallback over 443; local cursor and placement ghost in the viewer. Design the ingress outside Istio first. Acceptance: viewer connects from a UDP-blocked network via the fallback; click-to-photon p95 recorded for each transport.

**Phase 5: native-renderer spike (option B), time-boxed.** On one NVIDIA GPU: wgpu, two to eight views from one scene, NV12 conversion, encode through `gpu-video` H.264 and separately the NVENC export recipe; record render, convert and encode time, VRAM and fps at 1/4/8 views, and split render time into per-frame and per-pixel work. Size the Babylon migration. Acceptance: the decision gate in section 6.

**Phase 6: per-view cost reduction.** LOD and impostors for zoomed-out citizens, instanced crowds with baked animation, resolution and fps scaling for static scenes, shared stream for same-camera spectators, cheap or no shadows.

Cross-cutting: when a new path replaces JPEG, migrate viewer and bridge together and remove the old path unless the CPU fallback is a stated requirement; validate everything on the server including viewport and quota per player; keep the real-Chrome e2e and add regressions for slow-viewer frame drops, idle gating, and stale-chunk resubscription; create `tk` tasks per phase.

## 8. Decisions needed from the project owner

1. Target concurrent players at launch and GPU budget (home node vs rented vs cloud). Decides how much option B is worth.
2. Is a Rust wgpu renderer open to discussion, or is Babylon/Three fixed?
3. Is the CPU-only path required as a fallback for demos, or can it be removed once a GPU node exists?
4. Can the router forward a public UDP port, and is a public IP available for TURN?

## 9. Risks and evidence gaps

Risks: rendering may dominate cost so transport work buys little; Chromium has no NVENC on Linux so Option A needs an external encode stage; wgpu zero-copy needs unsafe version-pinned code; driver variance for Vulkan Video; Istio/Gateway API upgrade hazard; Cloudflare dependency if TURN is outsourced; unmeasured multi-view scaling.

Gaps: no latency threshold for city-builders; no primary-source latency numbers for commercial services; no per-citizen bandwidth from any real game; no instances-per-GPU numbers; no measurement of the shareable fraction of our frame cost. Phase 0, 2 and 5 exist to close the ones that matter.

## 10. Sources

Platforms: Sunshine https://raw.githubusercontent.com/LizardByte/Sunshine/master/docs/configuration.md; moonlight-common-c https://github.com/moonlight-stream/moonlight-common-c; Parsec BUD https://parsec.app/blog/a-networking-protocol-built-for-the-lowest-latency-interactive-game-streaming-1fd5a03a6007; Parsec browser https://parsec.app/blog/game-streaming-tech-in-the-browser-with-parsec-5b70d0f359bc; Stadia traffic https://arxiv.org/html/2009.09786v2, https://arxiv.org/abs/2012.06774.

Papers: Claypool 2006 https://dl.acm.org/doi/fullHtml/10.1145/1167838.1167860; Claypool mouse/delay https://doi.org/10.1145/3390464; frame rate/resolution https://web.cs.wpi.edu/~claypool/papers/fr-rez/paper.pdf; Chen latency https://people.cs.nycu.edu.tw/~chuang/pubs/pdf/2011mm.pdf; GCC https://doi.org/10.1145/2910017.2910605; Salsify https://www.usenix.org/conference/nsdi18/presentation/fouladi; Nebula https://arxiv.org/abs/2201.07738; codec comparison https://ieeexplore.ieee.org/document/7965686/; CAVE https://kdiab.ca/assets/pdf/pubs/mmsys19.pdf.

Shared rendering: Capsule https://arxiv.org/html/2506.11483; multi-view rendering for MMORPG https://dl.acm.org/doi/fullHtml/10.1145/3404397.3404463; indirect draws https://toji.dev/webgpu-best-practices/indirect-draws.html; crowd rendering https://developer.nvidia.com/gpugems/gpugems3/part-i-geometry/chapter-2-animated-crowd-rendering; Cities: Skylines 2 analysis https://news.ycombinator.com/item?id=38153573.

Native renderer: wgpu in Docker https://github.com/gfx-rs/wgpu/issues/2123; ICD fallback https://github.com/NVIDIA/nvidia-container-toolkit/issues/288; NVIDIA container capabilities https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/docker-specialized.html; gpu-video https://docs.rs/gpu-video/latest/gpu_video/; wgpu export requests https://github.com/gfx-rs/wgpu/issues/2320, https://github.com/gfx-rs/wgpu/issues/3145; wgpu to NVENC https://github.com/moq-dev/moq/pull/6805; Vulkan Video status https://blogs.igalia.com/vjaquez/vulkan-video-status/; AV1 encode https://www.khronos.org/blog/khronos-announces-vulkan-video-encode-av1-encode-quantization-map-extensions; pixelflux https://github.com/selkies-project/pixelflux; Sunshine docs https://docs.lizardbyte.dev/projects/sunshine/latest/index.html; Parsec NVENC telemetry https://parsec.app/blog/nvidia-nvenc-outperforms-amd-vce-on-h-264-encoding-latency-in-parsec-co-op-sessions-713b9e1e048a.

Chromium on GPU: Chrome headless GPU guidance https://developer.chrome.com/blog/supercharge-web-ai-testing; flags guide https://tigerabrodi.blog/how-to-get-webgpu-in-headless-chrome-on-cloud-gpus; headless screencast measurements https://github.com/pingdotgg/t3code/issues/26523; Electron VA-API limits https://github.com/electron/electron/issues/55456; Selkies https://github.com/selkies-project/selkies; GPU docs for Selkies https://docs.linuxserver.io/selkies/user-guide/gpu/; screencast ack https://chromium.googlesource.com/chromium/src/+/1c762fa1266fda75231b9ac7b515318fc5328b4d.

Replication: Gaffer snapshot compression https://gafferongames.com/post/snapshot_compression/, state sync https://gafferongames.com/post/state_synchronization/; Factorio FFF-76 https://www.factorio.com/blog/post/fff-76; Cities: Skylines Multiplayer PR https://github.com/CitiesSkylinesMultiplayer/CSM/pull/345; bevy_replicon https://github.com/projectharmonia/bevy_replicon; FlatBuffers benchmarks https://flatbuffers.dev/benchmarks/.

Ingress: Cloudflare Realtime TURN https://developers.cloudflare.com/realtime/turn/; LiveKit on Kubernetes https://docs.livekit.io/transport/self-hosting/kubernetes/; Istio UDP/Gateway API https://github.com/istio/istio/pull/62493, https://github.com/kubernetes-sigs/gateway-api/discussions/3475, https://kubernetes.io/blog/2026/08/03/gateway-api-v1-6-release/; Cloudflare UDP threads https://community.cloudflare.com/t/public-hostname-udp/452994; WebTransport behind Cloudflare https://community.cloudflare.com/t/is-webtransport-supported-by-cloudflare/587646.

Pricing: https://bex.co/blog/2026/07/13/hetzner-gex44-gpu-pricing-break-even, https://www.doit.com/compute/compute/aws/us-east-1/g6.xlarge.
