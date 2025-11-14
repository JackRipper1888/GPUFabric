#### XDP (eXpress Data Path) - Kernel-Level Packet Filtering
- **eBPF-based** packet processing at network driver level
- **Pre-kernel stack** filtering for ultra-low latency
- **API Key Validation** at kernel level before reaching user space
- **Location**: `/gpuf-s/src/xdp/`
- **Use Case**: High-performance request validation and DDoS protection

```bash
# Compile XDP program
cd gpuf-s/src/xdp
make deps  # First time only
make

# Load XDP filter
sudo ip link set dev <interface> xdp obj xdp_auth_filter.o sec xdp

# Add API key to XDP map
sudo bpftool map update name api_keys key hex 31 32 33 34 35 36 37 38 39 30 30 30 30 30 30 30 value hex 01
```