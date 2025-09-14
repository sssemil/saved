# Project: Personal “Saved Messages” Sync (P2P-first, Cloud-optional)

**Version:** 1.0
**Status:** Draft for implementation
**Primary Stack:** Rust + libp2p + SQLite + BLAKE3 + XChaCha20-Poly1305
**Authors:** —

---

## 0) TL;DR

A private, end‑to‑end encrypted personal vault that syncs notes, messages, and files across your own devices. It prefers **direct LAN** or **hole-punched** connections (DCUtR), and only falls back to **relay** when necessary. **No server required** to operate. A later, optional **zero‑knowledge cloud backup** adds durability, presence notifications, and your own relay/rendezvous, without learning anything about your data.

**Key properties**

* P2P by default (mDNS/LAN → direct → DCUtR → relay fallback).
* End‑to‑end encryption with account-scoped vault key.
* Append‑only event log + CRDT semantics for edits/deletes.
* Content‑addressed encrypted chunk store for files; dedup within the account.
* Optional zero‑knowledge Cloud Backup + Relay as a paid add‑on.
* Minimal to no servers required for users who don’t opt in.

```
+-----------+       +-----------+       +-----------+
|  Device A |<----->|  Device B |<----->|  Device C |
+-----------+       +-----------+       +-----------+
   |  LAN/mDNS           | DCUtR (hole-punch)   | Relay fallback
   |                     |                      |
   v                     v                      v
 [Direct QUIC]      [Direct QUIC]        [/p2p-circuit via Relay]

Optional later: Zero-knowledge Cloud Backup + Relay + Rendezvous
```

---

## 1) Goals & Non‑Goals

### Goals

* Provide a **private “Saved Messages”** experience syncing across personal devices with **no central server** required.
* Handle **text + attachments** (any file type), with **editing** and **deleting** semantics that converge across devices.
* Operate reliably across **NATs** using DCUtR and **Relay v2** fallback.
* Use **libp2p** for transport stack and discovery; keep dependencies tight and audited.
* Support an **optional, paid Cloud Backup** that remains zero‑knowledge (provider cannot read data) and reduces connection friction (rendezvous/relay/push-notify), while preserving P2P when possible.
* Offer a clean **core library API** that applications (CLI, desktop, mobile) can embed.

### Non‑Goals (v1)

* Multi‑user chat rooms or sharing across different accounts. (Future: optional sharing with explicit invites.)
* Rich collaborative editing (Google Docs‑style). We support message edits with CRDT conflict resolution but not arbitrary text OT.
* Public DHT discovery. We avoid global DHT for privacy; pairing via QR and remembered addresses is preferred.

---

## 2) Primary User Scenarios

1. **First Device Setup:** User initializes an Account (creates keys), starts listening. No servers involved.
2. **Link Second Device (No Internet):** Scan QR on LAN → mDNS + direct QUIC → sync both ways.
3. **Link Remote Device (Behind NATs):** Use QR (contains bootstrap relay multiaddrs) → DCUtR hole punching via relay → establish direct channel.
4. **Daily Use:** Add/edit/delete messages, attach files; devices announce new heads; others fetch diffs, converge.
5. **Cloud Backup Opt‑in (Later):** User buys subscription. Client uploads encrypted event log + encrypted blobs to Cloud Backup; provider can also serve as rendezvous/relay. Data remains E2E.

---

## 3) Architecture Overview

* **Identity:**

  * `AccountKey` (ed25519) – long‑lived, never leaves devices; signs device certificates.
  * `DeviceKey` (ed25519) – per device; authenticates in libp2p and signs ops.
  * `PeerId` = libp2p public key (from DeviceKey).
* **Vault:**

  * `K_vault` – 256‑bit symmetric key (random). Encrypts event payloads and chunk keys.
  * Stored locally encrypted by OS keystore when possible. Never uploaded unless user opts into a passphrase‑wrapped key backup.
* **Event Log (CRDT):**

  * Append‑only operation log per account: `Create`, `Edit`, `Delete`, `Attach`, `Detach`, etc.
  * Ops have IDs `OpID = (device_pubkey, counter)` and lamport timestamps; causality tracked via `parents[]`.
  * Converges deterministically; compaction can materialize state views.
* **Networking:**

  * Transport: **QUIC** preferred; **TCP** fallback; **Noise** handshake; **Yamux** mux.
  * Discovery: **mDNS** (LAN), **AddressBook** (paired peers), optional **Relay v2** addresses from QR/cloud.
  * NAT traversal: **AutoNAT + Identify + DCUtR**; **Relay v2** fallback for data as last resort.
  * Protocols: **Gossipsub** for head announcements; **Request‑Response** for ops/chunks.
* **Storage:**

  * Metadata/indices in **SQLite** (WAL mode, fsync on checkpoints).
  * Blobs in content‑addressed store under account directory; chunked, encrypted, deduped per account.

---

## 4) Networking Design (libp2p)

* **Transports:**

  * QUIC v1 with Noise: low‑latency, better NAT behavior, 0‑RTT resumption.
  * TCP + Noise + Yamux as fallback.
* **Behaviours:**

  * `mDNS` for LAN peer discovery.
  * `Identify` to exchange observed addrs.
  * `AutoNAT` to assess reachability.
  * `Relay v2 (client)` for coordination + fallback data.
  * `DCUtR` to punch holes using a reachable relay on both sides.
  * `Gossipsub` (signed) for `AnnounceHeads`.
  * `Request‑Response` for `FetchOps` and `FetchChunks`.
* **Discovery & Addressing:**

  * Avoid global DHT for privacy. Devices know each other via QR‑exchanged **bootstrap multiaddrs** (possibly including relay addrs) and persist **last‑seen addresses**.
  * A small list of community/public relays can ship with the app (opt‑in), but they are not required.
* **Connection Preference Order:**

  1. Direct on LAN (mDNS).
  2. Direct via public/AutoNAT‑observed addresses.
  3. DCUtR hole punching coordinated via relay.
  4. Relay data (\*/p2p‑circuit) as last resort.
* **Keepalive & Backoff:**

  * QUIC keepalive on idle streams; exponential backoff on dial failures per peer and per address.
* **Rate Limits:**

  * Per‑peer request window; per‑topic gossipsub publish rate; chunk transfer fairness (token bucket).

---

## 5) Data Model

### Core Entities

* **Account**: owns the vault; signs devices; holds `K_vault`.
* **Device**: a member of the account; has `DeviceKey` and local counters.
* **Feed**: a logical stream (e.g., “Saved Messages”); v1 ships with one default feed.
* **Message**: `{id, author_device, created_at, body, attachments[], edited_at?, deleted_at?}`.
* **Attachment**: references encrypted chunk CIDs.

### Event Types (append‑only)

* `CreateMessage { msg_id, feed_id, body, atts[], ts }`
* `EditMessage { msg_id, body_delta|body_full, ts }`
* `DeleteMessage { msg_id, ts, reason? }`
* `Attach { msg_id, att_ids[], ts }` / `Detach { msg_id, att_ids[], ts }`
* `Ack { up_to_op, ts }` (used for compaction/coalescing)

Each event includes: `OpID`, `lamport`, `parents[]`, and **E2E encrypted payload**.

### IDs & Clocks

* `OpID = (device_pubkey, u64_counter)`; counters monotonic per device.
* Lamport increments on each local op and on receive.
* `heads[]` is the frontier set of op hashes.

---

## 6) Cryptography & Key Management

* **Root keys:**

  * `AccountKey`: ed25519 (libsodium/ed25519-dalek). Used for device certs and optional signed metadata.
  * `DeviceKey`: ed25519. Used by libp2p Noise and to sign ops.
* **Vault key:**

  * `K_vault` (32 bytes) generated randomly at account creation.
  * Stored in OS keystore where possible (Keychain/Keystore). Else, protected with local passphrase via **Argon2id**.
* **Payload encryption:**

  * Event payloads encrypted with **XChaCha20-Poly1305** using `nonce = random(24B)`; key derived as `K_event = HKDF(K_vault, "event" || OpID)`.
  * Attachments chunk encryption uses **account‑scoped convergent encryption** for dedup within the account: `K_chunk = HKDF(K_vault, "chunk" || plaintext_hash)`. Ciphertext is stored under CID = hash(plaintext) to enable dedup across the user's devices while not revealing content to others.
* **Metadata protection:**

  * Event bodies and attachment filenames are encrypted. Minimal unencrypted metadata: op sizes, timing (network‑visible), and total account storage size. Optionally pad messages to buckets to reduce leakage (later).
* **Key rotation:**

  * Rotate `K_vault` by generating `K_vault'` and sealing the old to the new (store `wrap_old = AEAD(K_vault', K_vault)`). Re‑encrypt opportunistically; legacy chunks remain valid.
* **Device certificates:**

  * `DeviceCert = { device_pub, issued_at, expires?, signature_by_AccountKey }`.
* **Passphrase‑wrapped key backup (optional):**

  * Locally generate `K_backup = Argon2id(passphrase, salt, params)`; store `wrap = AEAD(K_backup, K_vault)` in Cloud Backup (optional). Provider never sees passphrase.

---

## 7) Sync Protocol (Account‑scoped)

### Topics & Protocol IDs

* Gossipsub topic: `/savedmsgs/<account_id>/heads`
* Request‑Response protocol IDs:

  * `/savedmsgs/1/ops`
  * `/savedmsgs/1/chunks`

### Messages

* `AnnounceHeads { feed_id, lamport, heads[] }` (gossipsub)
* `FetchOpsReq { since_heads[], want_max }` → `FetchOpsResp { ops[], new_heads[] }`
* `HaveChunksReq { cids[] }` → `HaveChunksResp { have_bitmap }`
* `FetchChunksReq { cids[] }` → `FetchChunksResp { {cid, chunk_ciphertext}[] }`

### State Machine (per peer)

1. **Idle** → receive `AnnounceHeads` → **NeedOps**.
2. **NeedOps**: diff frontier vs local; request missing via `FetchOpsReq` (paged).
3. **Merging**: validate signatures, decrypt payloads, apply CRDT; then identify missing chunk CIDs.
4. **NeedChunks**: query `HaveChunksReq` → batch `FetchChunksReq` until satisfied.
5. **Settled**: publish own `AnnounceHeads` if heads advanced.

### Validation

* Verify op signatures (DeviceKey).
* Enforce monotonic counters per device; reject forks unless explicitly allowed as CRDT merges.
* Reject ops that fail AEAD.

### Compaction & Checkpoints

* Periodically materialize a state snapshot (SQLite tables) with `checkpoint_hash` anchored into the log via a small op. Devices that possess a recent checkpoint can accelerate sync.

---

## 8) Editing & Deleting Semantics

* **Edits:** LWW register on the `body` field keyed by `(lamport, device_pubkey)` tie‑break; optional `body_delta` (later) for bandwidth saving. The log preserves history; UI exposes "edited" with timestamp.
* **Deletes:** `DeleteMessage` inserts a tombstone that hides the message in views. Tombstone wins over any earlier `Create`/`Edit` by LWW on `deleted_at`.
* **Redaction/Purge:** A `Purge { msg_id }` op can be issued to request physical deletion of message content and blobs. Peers honor purge when they see the op signed by AccountKey. (Cloud Backup also deletes ciphertext.) If an offline device never reconnects, purge can’t be enforced there—but new devices won’t receive purged data.

---

## 9) Attachments & Storage

* **Chunking:** Fixed 1–4 MiB chunks (configurable). CID = `BLAKE3(plaintext)`.
* **Encryption:** AEAD with `K_chunk` (see §6).
* **Dedup:** Within the account, identical plaintext chunks map to same CID; only one ciphertext is stored.
* **Resume:** Maintain per‑peer have‑lists; resume broken transfers by requesting missing CIDs.
* **GC:** Reference counts by message/attachment index. After `Detach`/`Delete`/`Purge`, decrement and remove when zero.
* **On‑disk layout:**

  * `db.sqlite` (WAL) – ops index, message/materialized views, refcounts.
  * `chunks/aa/bb/<cid>` – encrypted chunk blobs.
  * `keys/` – keystore (OS‑protected when possible).

---

## 10) Device Linking Flow (No Server Required)

1. **Initiator (existing device)** displays a QR containing:

   * `peer_id`, last‑known multiaddrs (LAN + relay if any), and a short‑lived `onboarding_token` signed by AccountKey (exp: \~10 min).
2. **Joiner** scans QR → dials addresses → Noise handshake → presents ephemeral key proving possession of `onboarding_token`.
3. **Mutual Auth:** Initiator verifies token (AccountKey signature). Joiner receives `DeviceCert` signed by AccountKey. Both exchange DeviceCerts.
4. **Key Transfer:** Over the established secure channel, Initiator transmits `K_vault` sealed to Joiner’s DeviceKey (X25519 sealed box or Noise export).
5. **Bootstrap:** Exchange address books, recent heads, and optional relay addresses.
6. **Persist:** Both save bootstrap info for future direct dials; no future server dependency.

*Note:* If no relay is available and both are behind symmetric NATs, users can walk over a local network or temporarily enable a public relay (or Cloud add‑on) just for the link step.

---

## 11) Cloud Backup (Optional Paid Add‑on)

**Principles:** Zero‑knowledge, minimal metadata, optional, and does not replace P2P.

**Capabilities:**

* **Encrypted Event Log Backup:** Append‑only upload of op ciphertexts and checkpoints. Provider stores only ciphertext + op hash, not keys.
* **Encrypted Chunk Backup:** Store chunk ciphertext by CID. Provider cannot derive plaintext.
* **Rendezvous/Relay:** Provider exposes a privacy‑aware rendezvous to exchange ephemeral peer hints and a high‑availability Relay v2 for DCUtR coordination. Data may still go direct.
* **Presence & Push:** Optional push notifications via provider when a device posts new heads (delivered as encrypted envelopes; provider cannot read content).

**Security & Auth:**

* Device authenticates with OAuth‑style token bound to the account, *not* to content. All uploaded data remains AEAD‑encrypted with keys the provider does not possess.
* Optional **passphrase‑wrapped key** backup (see §6) stored as opaque blob.

**Deletion:**

* Respect `Purge` ops: delete corresponding ciphertexts promptly.
* Account closure wipes all stored ciphertext and key‑wrap blobs.

**Business model:**

* Free tier: limited storage & relay bandwidth.
* Paid tier: more storage, prioritized relay, presence notifications, and faster rendezvous.

---

## 12) Privacy & Threat Model

* **Adversaries:**

  * Passive network observer.
  * Honest‑but‑curious Cloud provider (if used).
  * Malicious peer claiming to be your device.
  * Lost/stolen device attacker.
* **Mitigations:**

  * All content AEAD‑encrypted; Noise‑protected transport.
  * Ed25519 device certs signed by AccountKey; unknown devices rejected.
  * Convergent encryption scope limited to account to avoid cross‑user leakage.
  * Key material in OS keystore; lock screen/biometrics gate.
  * Optional remote device revoke op signed by AccountKey, preventing future sync.
* **Metadata minimization:** no global DHT; pairing via QR; only relay/rendezvous see connection timing and approximate sizes.

---

## 13) Failure Modes & Recovery

* **Both behind symmetric NAT & no relay:** linking requires temporary relay (Cloud or community).
* **Partial downloads:** resume via have‑list and op paging.
* **DB corruption/power loss:** SQLite WAL + periodic integrity checks; automatic snapshot/restore from latest checkpoint.
* **Clock skew:** lamport timestamps ensure convergence; wall clock only for UX.

---

## 14) Versioning & Compatibility

* Protocol IDs include major version (`/savedmsgs/1/...`).
* Protobuf messages use reserved fields for forward compatibility.
* Feature flags exchanged via Identify (e.g., supports\_delta\_edits, supports\_checkpoints\_v2).

---

## 15) Config & Tuning (defaults)

* Chunk size: 2 MiB.
* Ops page size: 256 ops.
* Max parallel chunk streams per peer: 4.
* Gossipsub mesh params tuned for small N (≤5 devices).
* QUIC idle timeout: 30s; keepalive: enabled.

---

## 16) Core Library API (Rust)

```rust
pub struct AccountHandle { /* opaque */ }
pub struct DeviceInfo { pub peer_id: PeerId, /* … */ }
pub struct MessageId(pub [u8; 32]);

pub struct Config {
    pub storage_path: PathBuf,
    pub allow_public_relays: bool,
    pub bootstrap_multiaddrs: Vec<Multiaddr>,
    pub use_kademlia: bool, // default false
}

pub enum Event {
    Connected(DeviceInfo),
    Disconnected(PeerId),
    HeadsUpdated,
    SyncProgress { done: u64, total: u64 },
}

impl AccountHandle {
    pub async fn create_or_open(cfg: Config) -> Result<Self>;
    pub async fn device_info(&self) -> DeviceInfo;

    // Device linking
    pub fn make_linking_qr(&self) -> QrPayload; // contains onboarding token, addrs
    pub async fn accept_link(&self, payload: QrPayload) -> Result<DeviceInfo>;

    // Messaging
    pub async fn create_message(&self, body: String, atts: Vec<PathBuf>) -> Result<MessageId>;
    pub async fn edit_message(&self, id: MessageId, new_body: String) -> Result<()>;
    pub async fn delete_message(&self, id: MessageId) -> Result<()>;
    pub async fn purge_message(&self, id: MessageId) -> Result<()>; // destructive

    // Sync
    pub async fn start_network(&self) -> Result<()>;
    pub async fn subscribe(&self) -> mpsc::Receiver<Event>;
    pub async fn force_announce(&self);

    // Backup (optional)
    pub async fn enable_cloud_backup(&self, endpoint: Url, token: String) -> Result<()>;
    pub async fn disable_cloud_backup(&self) -> Result<()>;
}
```

---

## 17) On‑Disk Layout

```
<account_dir>/
  db.sqlite        # WAL mode
  chunks/aa/bb/<cid>
  keys/
    device_key
    account_key.pub
    vault_key.sealed   # sealed by OS keystore or passphrase
  checkpoints/
    <epoch>.snap       # encrypted snapshot blobs
  addrbook.json        # last-seen multiaddrs (signed)
```

---

## 18) Testing & QA Plan

* **NAT Matrix:** Full pairwise across: Full‑cone, Restricted, Port‑restricted, Symmetric, CGNAT, dual‑stack IPv4/IPv6.
* **LAN Sleep/Wake:** Validate reconnect and incremental sync after device sleep.
* **Throughput:** ≥ 50 MB/s LAN, ≥ 5 MB/s typical WAN with QUIC, subject to network.
* **Fuzzing:** Protobuf and state machine fuzz tests; invalid ops/signatures; replay attacks.
* **Crash Consistency:** Kill during op apply and chunk writes; must recover without divergence.
* **Upgrade:** Downgrade/upgrade across protocol major/minor.

---

## 19) Rollout & Migration

* Start with CLI + desktop (macOS/Windows/Linux).
* Mobile apps later; reuse core library via FFI.
* Migration hooks to import/export plaintext archive (local only).

---

## 20) Future Work

* Selective partial sync (per tag/time range).
* Sharing with explicit trusted contacts (double‑ratchet sessions for shared feeds).
* Delta‑based edits for large texts.
* Encrypted search index with local query execution.
* Multi‑relay lists with performance scoring.

---

## Appendix A — Protobuf Schemas (v1)

```proto
syntax = "proto3";
package savedmsgs.v1;

message OpHeader {
  bytes op_id = 1;           // 40 bytes: 32 pubkey hash + 8 counter
  uint64 lamport = 2;        // logical time
  repeated bytes parents = 3;// op hashes
  bytes signer = 4;          // device pubkey
  bytes sig = 5;             // signature over (header || ciphertext)
}

message CreateMessageBody {
  bytes msg_id = 1; // 32B hash
  string feed_id = 2;
  string body = 3;  // plaintext before encryption; stored encrypted
  repeated bytes att_cids = 4; // chunk cids
  int64 created_at_ms = 5; // wall clock for UX
}

message EditMessageBody {
  bytes msg_id = 1;
  oneof payload {
    string body_full = 2;
    bytes body_delta = 3; // reserved for future
  }
  int64 edited_at_ms = 4;
}

message DeleteMessageBody {
  bytes msg_id = 1;
  string reason = 2;
  int64 deleted_at_ms = 3;
}

message AttachBody {
  bytes msg_id = 1;
  repeated bytes att_cids = 2;
}

message DetachBody {
  bytes msg_id = 1;
  repeated bytes att_cids = 2;
}

message OpEnvelope {
  OpHeader header = 1;
  bytes ciphertext = 2; // AEAD over serialized body
}

message AnnounceHeads {
  string feed_id = 1;
  uint64 lamport = 2;
  repeated bytes heads = 3; // op hashes
}

message FetchOpsReq {
  repeated bytes since_heads = 1;
  uint32 want_max = 2;
}
message FetchOpsResp {
  repeated OpEnvelope ops = 1;
  repeated bytes new_heads = 2;
}

message HaveChunksReq { repeated bytes cids = 1; }
message HaveChunksResp { bytes have_bitmap = 1; }

message FetchChunksReq { repeated bytes cids = 1; }
message FetchChunksResp {
  message Chunk { bytes cid = 1; bytes data = 2; }
  repeated Chunk chunks = 1;
}
```

---

## Appendix B — Protocol IDs

* Gossipsub topic: `/savedmsgs/<account_id>/heads`
* Request‑Response:

  * `/savedmsgs/1/ops`
  * `/savedmsgs/1/chunks`

---

## Appendix C — NAT Traversal Notes

* Enable `AutoNAT`, `Identify`, `Relay v2`, and `DCUtR` behaviours. Both peers should be connected to at least one relay to coordinate punching. Data shifts to direct QUIC upon success.
* If DCUtR fails (e.g., symmetric NAT both sides), use `/p2p-circuit` for data with flow control limits.

---

## Appendix D — Metrics (Local Only, Off by Default)

* `sync.ops_applied_total`, `sync.bytes_rx/tx`, `chunks.cache_hit_ratio`, `net.dial_success_rate`, `relay.usage_seconds`.
* Export locally to a file; no telemetry leaves device unless user explicitly opts in.
