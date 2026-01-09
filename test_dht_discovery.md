# WAN DHT Discovery Test (Acceptance Test 2)

## Goal
Verify nodes on different networks discover each other via Kademlia DHT using bootstrap peers.

## Prerequisites
- Docker and docker-compose installed
- Updated docker-compose.yml with bootstrap configuration

## Steps

### 1. Start the test environment
```bash
cd /Users/teoechavarria/Documents/GitHub/hybrid-connection-health
docker-compose up --build
```

### 2. Monitor logs
```bash
# In another terminal
docker-compose logs -f bootstrap client_auto1 client_auto2
```

## Expected Results (within 60 seconds)

### ✅ bootstrap container logs:
```
🆔 Local PeerId: 12D3KooW...
📡 Kademlia mode: Server (Gateway)
🎧 Listening on /ip4/0.0.0.0/tcp/4000
✅ Connection established with 12D3KooW... (client1)
🔍 Identified peer 12D3KooW...
🗺️ Kademlia routing updated: 12D3KooW... with 1 addresses
✅ Connection established with 12D3KooW... (client2)
🔍 Identified peer 12D3KooW...
🗺️ Kademlia routing updated: 12D3KooW... with 1 addresses
💚 Discovery health: connected=2, mdns_discovered=0, kad_discovered=2, ...
```

### ✅ client_auto1 and client_auto2 logs:
```
🆔 Local PeerId: 12D3KooW...
📡 Kademlia mode: Client
🔗 Dialing bootstrap peer: /ip4/172.20.0.10/tcp/4000/p2p/12D3KooW...
✅ Connection established with 12D3KooW... (bootstrap)
🔍 Identified peer 12D3KooW...
🌐 Bootstrapping Kademlia DHT...
✅ Kademlia bootstrap success with peer: 12D3KooW...
🗺️ Kademlia routing updated: 12D3KooW... (other client)
📞 Auto-dialing peer from Kademlia routing table: 12D3KooW...
✅ Connection established with 12D3KooW... (other client)
💚 Discovery health: connected=2, mdns_discovered=0, kad_discovered>=1, ...
```

## Verification Checklist

- [ ] Bootstrap node shows `connected=2`
- [ ] Each client shows `connected=2` (bootstrap + other client)
- [ ] `mdns_discovered=0` (Docker bridge doesn't support multicast)
- [ ] `kad_discovered>=1` on all nodes
- [ ] Kademlia bootstrap success logged
- [ ] Clients discover each other via DHT routing updates
- [ ] All connections automatic (no manual dial in container commands)

## Docker Networking Notes

**Default bridge network:**
- mDNS will NOT work (multicast not forwarded)
- DHT-based discovery is the primary mechanism
- Containers must reach bootstrap peer via static IP/DNS

**Custom network configuration:**
- Uses subnet 172.20.0.0/24
- Bootstrap has static IP: 172.20.0.10
- Clients use bootstrap IP in their config

## Troubleshooting

**Bootstrap connection fails:**
- Verify bootstrap container started first
- Check `/shared/bootstrap_peer_id` file exists
- Verify network connectivity: `docker exec client_auto1 ping 172.20.0.10`

**DHT bootstrap fails:**
- Check bootstrap peer ID matches in multiaddr
- Verify identify event received before bootstrap attempt
- Look for "Bootstrapping Kademlia DHT..." log message

**Clients don't discover each other:**
- Verify Kademlia routing updated events on bootstrap
- Check clients are in different containers (not same peer ID)
- Wait up to 60 seconds for DHT propagation

**No connections after discovery:**
- Check Docker network allows inter-container communication
- Verify firewall rules on host system
