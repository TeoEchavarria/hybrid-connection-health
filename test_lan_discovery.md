# LAN Auto-Mesh Test (Acceptance Test 1)

## Goal
Verify 3 nodes on same LAN discover each other via mDNS without any bootstrap configuration.

## Prerequisites
- 3 machines/terminals on same LAN (or 3 terminals on same machine)
- Firewall allows UDP 5353 (mDNS) and TCP ephemeral ports
- Compiled binary available

## Steps

### 1. Build the binary
```bash
cd /Users/teoechavarria/Documents/GitHub/hybrid-connection-health
cargo build --release
```

### 2. Terminal 1 - Gateway
```bash
./target/release/hybrid-connection-health run \
  --role gateway \
  --listen /ip4/0.0.0.0/tcp/4001 \
  --identity-file /tmp/gateway.key
```

### 3. Terminal 2 - Client 1
```bash
./target/release/hybrid-connection-health run \
  --role client \
  --listen /ip4/0.0.0.0/tcp/4002 \
  --identity-file /tmp/client1.key
```

### 4. Terminal 3 - Client 2
```bash
./target/release/hybrid-connection-health run \
  --role client \
  --listen /ip4/0.0.0.0/tcp/4003 \
  --identity-file /tmp/client2.key
```

## Expected Results (within 30 seconds)

### ✅ Terminal 1 logs (Gateway):
```
🆔 Local PeerId: 12D3KooW...
🎧 Listening on /ip4/0.0.0.0/tcp/4001
📡 mDNS Discovered: 12D3KooW... at /ip4/192.168.x.x/tcp/4002
📞 Auto-dialing mDNS peer: 12D3KooW...
✅ Connection established with 12D3KooW...
📡 mDNS Discovered: 12D3KooW... at /ip4/192.168.x.x/tcp/4003
📞 Auto-dialing mDNS peer: 12D3KooW...
✅ Connection established with 12D3KooW...
💚 Discovery health: connected=2, mdns_discovered=2, kad_discovered=0, uptime=...
```

### ✅ Terminal 2 & 3 logs (Clients):
```
🆔 Local PeerId: 12D3KooW...
🎧 Listening on /ip4/0.0.0.0/tcp/400X
📡 mDNS Discovered: 12D3KooW... at /ip4/192.168.x.x/tcp/4001
📞 Auto-dialing mDNS peer: 12D3KooW...
✅ Connection established with 12D3KooW...
📤 Sending OpSubmit to connected peer 12D3KooW...
📬 Received OpAck from 12D3KooW...
📡 mDNS Discovered: 12D3KooW... at /ip4/192.168.x.x/tcp/400Y
📞 Auto-dialing mDNS peer: 12D3KooW...
✅ Connection established with 12D3KooW...
💚 Discovery health: connected=2, mdns_discovered=2, ...
```

## Verification Checklist

- [ ] All nodes show `connected=2` in health check logs
- [ ] mDNS discovery events appear for all peers
- [ ] Connections establish automatically (no manual dial needed)
- [ ] Gateway node actively dials discovered peers (symmetric behavior)
- [ ] OpSubmit/OpAck messages exchanged successfully

## Troubleshooting

**No mDNS discoveries:**
- Check firewall isn't blocking UDP 5353
- Verify all nodes are on same subnet
- Try: `dns-sd -B _services._dns-sd._udp` to verify mDNS is working

**Connections fail after discovery:**
- Check firewall allows TCP connections
- Verify listen addresses are reachable

**Dial loops (same peer dialed repeatedly):**
- Check logs for dial backoff working (30-second cooldown)
- Should see: nodes don't dial same peer more than once per 30 seconds
